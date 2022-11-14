pub mod error;
pub mod geodataframe;
pub mod geoseries;
pub mod spatial_index;
mod util;

#[cfg(feature = "proj")]
pub mod proj;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
