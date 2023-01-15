use crate::enum_::GeometryType;
use crate::error::GeoArrowError;
use crate::trait_::GeometryArray;
use crate::MutablePointArray;
use geo::Point;
use polars::export::arrow::array::{Array, PrimitiveArray, StructArray};
use polars::export::arrow::bitmap::utils::{BitmapIter, ZipValidity};
use polars::export::arrow::bitmap::Bitmap;
use polars::export::arrow::buffer::Buffer;
use polars::export::arrow::datatypes::DataType;
use polars::prelude::ArrowField;

/// A [`GeometryArray`] semantically equivalent to `Vec<Option<Point>>` using Arrow's
/// in-memory representation.
#[derive(Debug, Clone)]
pub struct PointArray {
    x: Buffer<f64>,
    y: Buffer<f64>,
    validity: Option<Bitmap>,
}

pub(super) fn check(
    x: &[f64],
    y: &[f64],
    validity_len: Option<usize>,
) -> Result<(), GeoArrowError> {
    if validity_len.map_or(false, |len| len != x.len()) {
        return Err(GeoArrowError::General(
            "validity mask length must match the number of values".to_string(),
        ));
    }

    if x.len() != y.len() {
        return Err(GeoArrowError::General(
            "x and y arrays must have the same length".to_string(),
        ));
    }
    Ok(())
}

impl PointArray {
    /// Create a new PointArray from parts
    /// # Implementation
    /// This function is `O(1)`.
    pub fn new(x: Buffer<f64>, y: Buffer<f64>, validity: Option<Bitmap>) -> Self {
        check(&x, &y, validity.as_ref().map(|v| v.len())).unwrap();
        Self { x, y, validity }
    }

    /// Create a new PointArray from parts
    /// # Implementation
    /// This function is `O(1)`.
    pub fn try_new(
        x: Buffer<f64>,
        y: Buffer<f64>,
        validity: Option<Bitmap>,
    ) -> Result<Self, GeoArrowError> {
        check(&x, &y, validity.as_ref().map(|v| v.len()))?;
        Ok(Self { x, y, validity })
    }

    /// Returns the number of geometries in this array
    #[inline]
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// Returns true if the array is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The values [`Buffer`].
    /// Values on null slots are undetermined (they can be anything).
    #[inline]
    pub fn values_x(&self) -> &Buffer<f64> {
        &self.x
    }

    /// The values [`Buffer`].
    /// Values on null slots are undetermined (they can be anything).
    #[inline]
    pub fn values_y(&self) -> &Buffer<f64> {
        &self.y
    }

    /// Returns the optional validity.
    #[inline]
    pub fn validity(&self) -> Option<&Bitmap> {
        self.validity.as_ref()
    }

    /// Returns a clone of this [`PrimitiveArray`] sliced by an offset and length.
    /// # Implementation
    /// This operation is `O(1)` as it amounts to increase two ref counts.
    /// # Examples
    /// ```
    /// use arrow2::array::PrimitiveArray;
    ///
    /// let array = PrimitiveArray::from_vec(vec![1, 2, 3]);
    /// assert_eq!(format!("{:?}", array), "Int32[1, 2, 3]");
    /// let sliced = array.slice(1, 1);
    /// assert_eq!(format!("{:?}", sliced), "Int32[2]");
    /// // note: `sliced` and `array` share the same memory region.
    /// ```
    /// # Panic
    /// This function panics iff `offset + length > self.len()`.
    #[inline]
    #[must_use]
    pub fn slice(&self, offset: usize, length: usize) -> Self {
        assert!(
            offset + length <= self.len(),
            "offset + length may not exceed length of array"
        );
        unsafe { self.slice_unchecked(offset, length) }
    }

    /// Returns a clone of this [`PrimitiveArray`] sliced by an offset and length.
    /// # Implementation
    /// This operation is `O(1)` as it amounts to increase two ref counts.
    /// # Safety
    /// The caller must ensure that `offset + length <= self.len()`.
    #[inline]
    #[must_use]
    pub unsafe fn slice_unchecked(&self, offset: usize, length: usize) -> Self {
        let validity = self
            .validity
            .clone()
            .map(|bitmap| bitmap.slice_unchecked(offset, length))
            .and_then(|bitmap| (bitmap.unset_bits() > 0).then_some(bitmap));
        Self {
            x: self.x.clone().slice_unchecked(offset, length),
            y: self.y.clone().slice_unchecked(offset, length),
            validity,
        }
    }
}

// Implement geometry accessors
impl PointArray {
    /// Returns the value at slot `i` as a geo object.
    pub fn value_as_geo(&self, i: usize) -> Point {
        Point::new(self.x[i], self.y[i])
    }

    /// Gets the value at slot `i` as a geo object, additionally checking the validity bitmap
    pub fn get_as_geo(&self, i: usize) -> Option<Point> {
        if self.is_null(i) {
            return None;
        }

        Some(self.value_as_geo(i))
    }

    /// Iterator over geo Geometry objects, not looking at validity
    pub fn iter_geo_values(&self) -> impl Iterator<Item = Point> + '_ {
        (0..self.len()).map(|i| self.value_as_geo(i))
    }

    /// Iterator over geo Geometry objects, taking into account validity
    pub fn iter_geo(&self) -> ZipValidity<Point, impl Iterator<Item = Point> + '_, BitmapIter> {
        ZipValidity::new_with_validity(self.iter_geo_values(), self.validity())
    }

    /// Returns the value at slot `i` as a GEOS geometry.
    #[cfg(feature = "geos")]
    pub fn value_as_geos(&self, i: usize) -> geos::Geometry {
        (&self.value_as_geo(i)).try_into().unwrap()
    }

    /// Gets the value at slot `i` as a GEOS geometry, additionally checking the validity bitmap
    #[cfg(feature = "geos")]
    pub fn get_as_geos(&self, i: usize) -> Option<geos::Geometry> {
        if self.is_null(i) {
            return None;
        }

        self.get_as_geo(i).as_ref().map(|g| g.try_into().unwrap())
    }

    /// Iterator over GEOS geometry objects
    #[cfg(feature = "geos")]
    pub fn iter_geos_values(&self) -> impl Iterator<Item = geos::Geometry> + '_ {
        (0..self.len()).map(|i| self.value_as_geos(i))
    }

    /// Iterator over GEOS geometry objects, taking validity into account
    #[cfg(feature = "geos")]
    pub fn iter_geos(
        &self,
    ) -> ZipValidity<geos::Geometry, impl Iterator<Item = geos::Geometry> + '_, BitmapIter> {
        ZipValidity::new_with_validity(self.iter_geos_values(), self.validity())
    }

    pub fn into_arrow(self) -> StructArray {
        let field_x = ArrowField::new("x", DataType::Float64, false);
        let field_y = ArrowField::new("y", DataType::Float64, false);

        let array_x =
            Box::new(PrimitiveArray::new(DataType::Float64, self.x, None)) as Box<dyn Array>;
        let array_y =
            Box::new(PrimitiveArray::new(DataType::Float64, self.y, None)) as Box<dyn Array>;

        let struct_data_type = DataType::Struct(vec![field_x, field_y]);
        let struct_values = vec![array_x, array_y];

        let validity: Option<Bitmap> = if let Some(validity) = self.validity {
            validity.into()
        } else {
            None
        };

        StructArray::new(struct_data_type, struct_values, validity)
    }
}

impl TryFrom<StructArray> for PointArray {
    type Error = GeoArrowError;

    fn try_from(value: StructArray) -> Result<Self, Self::Error> {
        let arrays = value.values();
        let validity = value.validity();

        if !arrays.len() == 2 {
            return Err(GeoArrowError::General(
                "Expected two child arrays of this StructArray.".to_string(),
            ));
        }

        let x_array_values = arrays[0]
            .as_any()
            .downcast_ref::<PrimitiveArray<f64>>()
            .unwrap();
        let y_array_values = arrays[1]
            .as_any()
            .downcast_ref::<PrimitiveArray<f64>>()
            .unwrap();

        Ok(Self::new(
            x_array_values.values().clone(),
            y_array_values.values().clone(),
            validity.cloned(),
        ))
    }
}

impl From<PointArray> for StructArray {
    fn from(value: PointArray) -> Self {
        let field_x = ArrowField::new("x", DataType::Float64, false);
        let field_y = ArrowField::new("y", DataType::Float64, false);

        let array_x = PrimitiveArray::<f64>::new(DataType::Float64, value.x, None);
        let array_y = PrimitiveArray::<f64>::new(DataType::Float64, value.y, None);

        let struct_data_type = DataType::Struct(vec![field_x, field_y]);
        let struct_values: Vec<Box<dyn Array>> = vec![array_x.boxed(), array_y.boxed()];

        let validity: Option<Bitmap> = if let Some(validity) = value.validity {
            validity.into()
        } else {
            None
        };

        StructArray::new(struct_data_type, struct_values, validity)
    }
}

impl GeometryArray for PointArray {
    #[inline]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    #[inline]
    fn len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn geometry_type(&self) -> GeometryType {
        GeometryType::WKB
    }

    fn validity(&self) -> Option<&Bitmap> {
        self.validity()
    }

    fn slice(&self, offset: usize, length: usize) -> Box<dyn GeometryArray> {
        Box::new(self.slice(offset, length))
    }

    unsafe fn slice_unchecked(&self, offset: usize, length: usize) -> Box<dyn GeometryArray> {
        Box::new(self.slice_unchecked(offset, length))
    }

    fn to_boxed(&self) -> Box<dyn GeometryArray> {
        Box::new(self.clone())
    }
}

impl From<Vec<Option<Point>>> for PointArray {
    fn from(other: Vec<Option<Point>>) -> Self {
        let mut_arr: MutablePointArray = other.into();
        mut_arr.into()
    }
}

impl From<Vec<Point>> for PointArray {
    fn from(other: Vec<Point>) -> Self {
        let mut_arr: MutablePointArray = other.into();
        mut_arr.into()
    }
}
