use crate::enum_::GeometryType;
use crate::error::GeoArrowError;
use crate::trait_::GeometryArray;
use geo::{Coord, LineString};
use polars::export::arrow::array::{ListArray, PrimitiveArray, StructArray};
use polars::export::arrow::bitmap::utils::{BitmapIter, ZipValidity};
use polars::export::arrow::bitmap::Bitmap;
use polars::export::arrow::buffer::Buffer;
use polars::export::arrow::offset::OffsetsBuffer;

/// A [`GeometryArray`] semantically equivalent to `Vec<Option<LineString>>` using Arrow's
/// in-memory representation.
#[derive(Debug, Clone)]
pub struct LineStringArray {
    /// Buffer of x coordinates
    x: Buffer<f64>,

    /// Buffer of y coordinates
    y: Buffer<f64>,

    /// Offsets into the coordinate array where each geometry starts
    geom_offsets: OffsetsBuffer<i64>,

    /// Validity bitmap
    validity: Option<Bitmap>,
}

pub(super) fn check(
    x: &[f64],
    y: &[f64],
    validity_len: Option<usize>,
) -> Result<(), GeoArrowError> {
    // TODO: check geom offsets?
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

impl LineStringArray {
    /// Create a new LineStringArray from parts
    /// # Implementation
    /// This function is `O(1)`.
    pub fn new(
        x: Buffer<f64>,
        y: Buffer<f64>,
        geom_offsets: OffsetsBuffer<i64>,
        validity: Option<Bitmap>,
    ) -> Self {
        check(&x, &y, validity.as_ref().map(|v| v.len())).unwrap();
        Self {
            x,
            y,
            geom_offsets,
            validity,
        }
    }

    /// Create a new LineStringArray from parts
    /// # Implementation
    /// This function is `O(1)`.
    pub fn try_new(
        x: Buffer<f64>,
        y: Buffer<f64>,
        geom_offsets: OffsetsBuffer<i64>,
        validity: Option<Bitmap>,
    ) -> Result<Self, GeoArrowError> {
        check(&x, &y, validity.as_ref().map(|v| v.len()))?;
        Ok(Self {
            x,
            y,
            geom_offsets,
            validity,
        })
    }

    /// Returns the number of geometries in this array
    #[inline]
    pub fn len(&self) -> usize {
        self.geom_offsets.len() - 1
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
            .and_then(|bitmap| (bitmap.unset_bits() > 0).then(|| bitmap));
        Self {
            x: self.x.clone().slice_unchecked(offset, length),
            y: self.y.clone().slice_unchecked(offset, length),
            geom_offsets: self.geom_offsets.clone().slice_unchecked(offset, length),
            validity,
        }
    }
}

// Implement geometry accessors
impl LineStringArray {
    /// Returns the value at slot `i` as a geo object.
    pub fn value_as_geo(&self, i: usize) -> LineString {
        let (start_idx, end_idx) = self.geom_offsets.start_end(i);
        let mut coords: Vec<Coord> = Vec::with_capacity(end_idx - start_idx);

        for i in start_idx..end_idx {
            coords.push(Coord {
                x: self.x[i],
                y: self.y[i],
            })
        }

        LineString::new(coords)
    }

    /// Gets the value at slot `i` as a geo object, additionally checking the validity bitmap
    pub fn get_as_geo(&self, i: usize) -> Option<LineString> {
        if self.is_null(i) {
            return None;
        }

        Some(self.value_as_geo(i))
    }

    /// Iterator over geo Geometry objects, not looking at validity
    fn iter_geo_values(&self) -> impl Iterator<Item = LineString> + '_ {
        (0..self.len()).map(|i| self.value_as_geo(i))
    }

    /// Iterator over geo Geometry objects, taking into account validity
    fn iter_geo(
        &self,
    ) -> ZipValidity<LineString, impl Iterator<Item = LineString> + '_, BitmapIter> {
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
    fn iter_geos_values(&self) -> impl Iterator<Item = geos::Geometry> + '_ {
        (0..self.len()).map(|i| self.value_as_geos(i))
    }

    /// Iterator over GEOS geometry objects, taking validity into account
    #[cfg(feature = "geos")]
    fn iter_geos(
        &self,
    ) -> ZipValidity<geos::Geometry, impl Iterator<Item = geos::Geometry> + '_, BitmapIter> {
        ZipValidity::new_with_validity(self.iter_geos_values(), self.validity())
    }
}

impl TryFrom<ListArray<i64>> for LineStringArray {
    type Error = GeoArrowError;

    fn try_from(value: ListArray<i64>) -> Result<Self, Self::Error> {
        let inner_dyn_array = value.values();
        let struct_array = inner_dyn_array
            .as_any()
            .downcast_ref::<StructArray>()
            .unwrap();
        let geom_offsets = value.offsets();
        let validity = value.validity();

        let x_array_values = struct_array.values()[0]
            .as_any()
            .downcast_ref::<PrimitiveArray<f64>>()
            .unwrap();
        let y_array_values = struct_array.values()[1]
            .as_any()
            .downcast_ref::<PrimitiveArray<f64>>()
            .unwrap();

        Ok(Self::new(
            x_array_values.values().clone(),
            y_array_values.values().clone(),
            geom_offsets.clone(),
            validity.cloned(),
        ))
    }
}

impl GeometryArray for LineStringArray {
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
