use super::MutableMultiPointArray;
use crate::enum_::GeometryType;
use crate::error::GeoArrowError;
use crate::trait_::GeometryArray;
use crate::LineStringArray;
use arrow2::array::{Array, ListArray, PrimitiveArray, StructArray};
use arrow2::bitmap::utils::{BitmapIter, ZipValidity};
use arrow2::bitmap::Bitmap;
use arrow2::buffer::Buffer;
use arrow2::offset::OffsetsBuffer;
use geozero::{GeomProcessor, GeozeroGeometry};

/// A [`GeometryArray`] semantically equivalent to `Vec<Option<MultiPoint>>` using Arrow's
/// in-memory representation.
#[derive(Debug, Clone)]
pub struct MultiPointArray {
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
    geom_offsets: &OffsetsBuffer<i64>,
) -> Result<(), GeoArrowError> {
    // TODO: check geom offsets?
    if validity_len.map_or(false, |len| len != geom_offsets.len()) {
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

impl MultiPointArray {
    /// Create a new MultiPointArray from parts
    /// # Implementation
    /// This function is `O(1)`.
    pub fn new(
        x: Buffer<f64>,
        y: Buffer<f64>,
        geom_offsets: OffsetsBuffer<i64>,
        validity: Option<Bitmap>,
    ) -> Self {
        check(&x, &y, validity.as_ref().map(|v| v.len()), &geom_offsets).unwrap();
        Self {
            x,
            y,
            geom_offsets,
            validity,
        }
    }

    /// Create a new MultiPointArray from parts
    /// # Implementation
    /// This function is `O(1)`.
    pub fn try_new(
        x: Buffer<f64>,
        y: Buffer<f64>,
        geom_offsets: OffsetsBuffer<i64>,
        validity: Option<Bitmap>,
    ) -> Result<Self, GeoArrowError> {
        check(&x, &y, validity.as_ref().map(|v| v.len()), &geom_offsets)?;
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
        self.geom_offsets.len()
    }

    /// Returns true if the array is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
            geom_offsets: self.geom_offsets.clone().slice_unchecked(offset, length),
            validity,
        }
    }
}

// Implement geometry accessors
impl MultiPointArray {
    pub fn value(&self, i: usize) -> crate::MultiPoint {
        crate::MultiPoint {
            x: &self.x,
            y: &self.y,
            geom_offsets: &self.geom_offsets,
            geom_index: i,
        }
    }

    pub fn get(&self, i: usize) -> Option<crate::MultiPoint> {
        if self.is_null(i) {
            return None;
        }

        Some(self.value(i))
    }

    /// Returns the value at slot `i` as a geo object.
    pub fn value_as_geo(&self, i: usize) -> geo::MultiPoint {
        self.value(i).into()
    }

    /// Gets the value at slot `i` as a geo object, additionally checking the validity bitmap
    pub fn get_as_geo(&self, i: usize) -> Option<geo::MultiPoint> {
        if self.is_null(i) {
            return None;
        }

        Some(self.value_as_geo(i))
    }

    /// Iterator over geo Geometry objects, not looking at validity
    pub fn iter_geo_values(&self) -> impl Iterator<Item = geo::MultiPoint> + '_ {
        (0..self.len()).map(|i| self.value_as_geo(i))
    }

    /// Iterator over geo Geometry objects, taking into account validity
    pub fn iter_geo(
        &self,
    ) -> ZipValidity<geo::MultiPoint, impl Iterator<Item = geo::MultiPoint> + '_, BitmapIter> {
        ZipValidity::new_with_validity(self.iter_geo_values(), self.validity())
    }

    // GEOS from not implemented for MultiPoint?!?
    //
    // /// Returns the value at slot `i` as a GEOS geometry.
    // #[cfg(feature = "geos")]
    // pub fn value_as_geos(&self, i: usize) -> geos::Geometry {
    //     (&self.value_as_geo(i)).try_into().unwrap()
    // }

    // /// Gets the value at slot `i` as a GEOS geometry, additionally checking the validity bitmap
    // #[cfg(feature = "geos")]
    // pub fn get_as_geos(&self, i: usize) -> Option<geos::Geometry> {
    //     if self.is_null(i) {
    //         return None;
    //     }

    //     self.get_as_geo(i).as_ref().map(|g| g.try_into().unwrap())
    // }

    // /// Iterator over GEOS geometry objects
    // #[cfg(feature = "geos")]
    // pub fn iter_geos_values(&self) -> impl Iterator<Item = geos::Geometry> + '_ {
    //     (0..self.len()).map(|i| self.value_as_geos(i))
    // }

    // /// Iterator over GEOS geometry objects, taking validity into account
    // #[cfg(feature = "geos")]
    // pub fn iter_geos(
    //     &self,
    // ) -> ZipValidity<geos::Geometry, impl Iterator<Item = geos::Geometry> + '_, BitmapIter> {
    //     ZipValidity::new_with_validity(self.iter_geos_values(), self.validity())
    // }

    pub fn into_arrow(self) -> ListArray<i64> {
        let linestring_array: LineStringArray = self.into();
        linestring_array.into_arrow()
    }
}

impl TryFrom<ListArray<i64>> for MultiPointArray {
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

impl TryFrom<Box<dyn Array>> for MultiPointArray {
    type Error = GeoArrowError;

    fn try_from(value: Box<dyn Array>) -> Result<Self, Self::Error> {
        let arr = value.as_any().downcast_ref::<ListArray<i64>>().unwrap();
        arr.clone().try_into()
    }
}

impl GeometryArray for MultiPointArray {
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

impl From<Vec<Option<geo::MultiPoint>>> for MultiPointArray {
    fn from(other: Vec<Option<geo::MultiPoint>>) -> Self {
        let mut_arr: MutableMultiPointArray = other.into();
        mut_arr.into()
    }
}

impl From<Vec<geo::MultiPoint>> for MultiPointArray {
    fn from(other: Vec<geo::MultiPoint>) -> Self {
        let mut_arr: MutableMultiPointArray = other.into();
        mut_arr.into()
    }
}

/// LineString and MultiPoint have the same layout, so enable conversions between the two to change
/// the semantic type
impl From<MultiPointArray> for LineStringArray {
    fn from(value: MultiPointArray) -> Self {
        Self::new(value.x, value.y, value.geom_offsets, value.validity)
    }
}

impl GeozeroGeometry for MultiPointArray {
    fn process_geom<P: GeomProcessor>(&self, processor: &mut P) -> geozero::error::Result<()>
    where
        Self: Sized,
    {
        let num_geometries = self.len();
        processor.geometrycollection_begin(num_geometries, 0)?;

        for geom_idx in 0..num_geometries {
            let (start_coord_idx, end_coord_idx) = self.geom_offsets.start_end(geom_idx);

            processor.multipoint_begin(end_coord_idx - start_coord_idx, geom_idx)?;

            for coord_idx in start_coord_idx..end_coord_idx {
                processor.xy(
                    self.x[coord_idx],
                    self.y[coord_idx],
                    coord_idx - start_coord_idx,
                )?;
            }

            processor.multipoint_end(geom_idx)?;
        }

        processor.geometrycollection_end(num_geometries - 1)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use geo::{point, MultiPoint};
    use geozero::ToWkt;

    fn mp0() -> MultiPoint {
        MultiPoint::new(vec![
            point!(
                x: 0., y: 1.
            ),
            point!(
                x: 1., y: 2.
            ),
        ])
    }

    fn mp1() -> MultiPoint {
        MultiPoint::new(vec![
            point!(
                x: 3., y: 4.
            ),
            point!(
                x: 5., y: 6.
            ),
        ])
    }

    #[test]
    fn geo_roundtrip_accurate() {
        let arr: MultiPointArray = vec![mp0(), mp1()].into();
        assert_eq!(arr.value_as_geo(0), mp0());
        assert_eq!(arr.value_as_geo(1), mp1());
    }

    #[test]
    fn geo_roundtrip_accurate_option_vec() {
        let arr: MultiPointArray = vec![Some(mp0()), Some(mp1()), None].into();
        assert_eq!(arr.get_as_geo(0), Some(mp0()));
        assert_eq!(arr.get_as_geo(1), Some(mp1()));
        assert_eq!(arr.get_as_geo(2), None);
    }

    #[test]
    fn geozero_process_geom() -> geozero::error::Result<()> {
        let arr: MultiPointArray = vec![mp0(), mp1()].into();
        let wkt = arr.to_wkt()?;
        let expected = "GEOMETRYCOLLECTION(MULTIPOINT(0 1,1 2),MULTIPOINT(3 4,5 6))";
        assert_eq!(wkt, expected);
        Ok(())
    }
}
