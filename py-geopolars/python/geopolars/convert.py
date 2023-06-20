from __future__ import annotations

from typing import TYPE_CHECKING

import polars as pl
from polars import DataFrame, Series

from geopolars.internals.geodataframe import GeoDataFrame
from geopolars.internals.geoseries import GeoSeries

try:
    import geopandas
except ImportError:
    geopandas = None

try:
    import pyarrow
except ImportError:
    pyarrow = None

try:
    import shapely
except ImportError:
    shapely = None


if TYPE_CHECKING:  # pragma: no cover
    import geopandas
    import pandas
    import pyarrow as pa


def from_arrow(a: pa.Table | pa.Array | pa.ChunkedArray) -> GeoDataFrame | GeoSeries:
    """
    Construct a GeoPolars `GeoDataFrame` or `GeoSeries` from an
    Arrow Table or Array.

    This operation will be zero copy for the most part. Types that are not
    supported by Polars may be cast to the closest supported type.

    Parameters:
        a: Data represented as Arrow Table or Array.

    Returns:
        `GeoDataFrame` or `GeoSeries`
    """
    # TODO: this should probably have a check that the data is indeed geographic?
    # And return a bare Series/DataFrame if it isn't?
    output = pl.from_arrow(a)
    if isinstance(output, Series):
        return GeoSeries(output)

    assert isinstance(
        output, DataFrame
    ), "Output of polars.from_arrow expected to be Series or DataFrame"
    return GeoDataFrame(output)


def from_geopandas(
    gdf: geopandas.GeoDataFrame
    | geopandas.GeoSeries
    | pandas.DataFrame
    | pandas.Series,
    force_wkb: bool = True,
) -> GeoDataFrame | GeoSeries | DataFrame | Series:
    """
    Construct a GeoPolars `GeoDataFrame` or `GeoSeries` from a
    [`geopandas.GeoDataFrame`][geopandas.GeoDataFrame] or
    [`geopandas.GeoSeries`][geopandas.GeoSeries].

    This operation clones data.

    This requires that `geopandas` and `pyarrow` are installed.

    Parameters:

        gdf: Input data to convert from.

    Returns:

        A GeoPolars `GeoDataFrame` or `GeoSeries`
    """
    if geopandas is None:
        raise ImportError("Geopandas is required when using from_geopandas().")

    import pandas

    if isinstance(gdf, geopandas.GeoSeries):
        return GeoSeries._from_geopandas(gdf, force_wkb=force_wkb)
    elif isinstance(gdf, geopandas.GeoDataFrame):
        return GeoDataFrame._from_geopandas(gdf, force_wkb=force_wkb)
    elif isinstance(gdf, (pandas.DataFrame, pandas.Series)):
        return pl.from_pandas(gdf)
    else:
        raise ValueError(
            f"Expected geopandas GeoDataFrame or GeoSeries, got {type(gdf)}."
        )
