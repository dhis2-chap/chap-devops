#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["duckdb", "numpy", "pandas", "pyarrow"]
# ///
"""Measure the size of one synthetic Chap dataset in each format, and the time to read a slice.

    scripts/measure-dataset-formats.py OUT_DIR

The script makes two datasets with random values:

- facility: 1,500 org units, 366 weeks, 4 features (2.2 M values);
- national: 2,000 org units, 1,040 weeks, 10 features (20.8 M values).

For each, it writes JSON (one object for each value, and columnar), CSV and
Parquet (wide and long, zstd) to OUT_DIR, and prints the sizes, with gzip. It
then reads two years and three columns from the wide Parquet file with pyarrow,
sums the cases for each period in that slice with DuckDB, and prints the times.

Real data is smoother than random values and usually compresses better.
The results are in the Chap dataset API notes of 2026-11-09 (CLIM-1786).
"""

import gzip
import io
import json
import sys
import time
from datetime import date, timedelta
from pathlib import Path

import duckdb
import numpy as np
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq

if len(sys.argv) != 2 or sys.argv[1].startswith("-"):
    sys.exit("usage: scripts/measure-dataset-formats.py OUT_DIR")
out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)


def build(n_units: int, n_weeks: int, n_features: int, seed: int = 1) -> pd.DataFrame:
    rng = np.random.default_rng(seed)
    start = date(2004, 1, 5)
    weeks = [start + timedelta(weeks=i) for i in range(n_weeks)]
    period_ids = [f"{d.isocalendar()[0]}W{d.isocalendar()[1]:02d}" for d in weeks]
    units = [f"ou{rng.integers(10**9):010d}"[:11] for _ in range(n_units)]
    n = n_units * n_weeks
    frame = pd.DataFrame(
        {
            "org_unit": np.repeat(units, n_weeks),
            "period": np.tile(period_ids, n_units),
            "period_start": np.tile(np.array(weeks, dtype="datetime64[D]"), n_units),
            "disease_cases": rng.poisson(20, n).astype("float64"),
        }
    )
    for i in range(n_features - 1):
        frame[f"covariate_{i}"] = np.round(rng.normal(25, 5, n), 2)
    return frame.sort_values(["period_start", "org_unit"], kind="stable").reset_index(drop=True)


def mb(n_bytes: int) -> float:
    return round(n_bytes / 1e6, 1)


def measure(label: str, frame: pd.DataFrame, slice_from: date, slice_to: date) -> dict:
    features = [c for c in frame.columns if c not in ("org_unit", "period", "period_start")]
    long = frame.melt(id_vars=["org_unit", "period"], value_vars=features, var_name="featureName", value_name="value")
    result = {"label": label, "rows": len(frame), "observations": len(long)}

    if len(long) < 5_000_000:
        long_json = json.dumps(
            [{"orgUnit": o, "period": p, "featureName": f, "value": v} for o, p, f, v in long.itertuples(index=False)]
        ).encode()
        result["json_long_mb"] = mb(len(long_json))
        result["json_long_gz_mb"] = mb(len(gzip.compress(long_json, 6)))
        del long_json
    col_json = json.dumps(
        {
            "orgUnit": frame["org_unit"].tolist(),
            "period": frame["period"].tolist(),
            "values": {f: frame[f].tolist() for f in features},
        }
    ).encode()
    result["json_columnar_mb"] = mb(len(col_json))
    result["json_columnar_gz_mb"] = mb(len(gzip.compress(col_json, 6)))
    csv = frame.drop(columns=["period_start"]).to_csv(index=False).encode()
    result["csv_mb"] = mb(len(csv))
    result["csv_gz_mb"] = mb(len(gzip.compress(csv, 6)))

    table = pa.Table.from_pandas(frame, preserve_index=False)
    wide_path = out / f"{label}_wide.parquet"
    pq.write_table(table, wide_path, compression="zstd", row_group_size=200_000)
    result["parquet_wide_mb"] = mb(wide_path.stat().st_size)
    long_path = out / f"{label}_long.parquet"
    pq.write_table(pa.Table.from_pandas(long, preserve_index=False), long_path, compression="zstd", row_group_size=200_000)
    result["parquet_long_mb"] = mb(long_path.stat().st_size)

    t = time.perf_counter()
    pq.read_table(wide_path).to_pandas()
    result["read_all_s"] = round(time.perf_counter() - t, 3)

    lo, hi = pa.scalar(slice_from, pa.date32()), pa.scalar(slice_to, pa.date32())
    t = time.perf_counter()
    sliced = ds.dataset(wide_path).to_table(
        columns=["org_unit", "period", "disease_cases"],
        filter=(ds.field("period_start").cast(pa.date32()) >= lo) & (ds.field("period_start").cast(pa.date32()) <= hi),
    )
    result["slice_pyarrow_s"] = round(time.perf_counter() - t, 3)
    result["slice_rows"] = sliced.num_rows

    t = time.perf_counter()
    duckdb.sql(
        f"SELECT period, sum(disease_cases) FROM read_parquet('{wide_path}') "
        f"WHERE period_start BETWEEN DATE '{slice_from}' AND DATE '{slice_to}' GROUP BY period ORDER BY period"
    ).fetchall()
    result["sum_per_period_duckdb_s"] = round(time.perf_counter() - t, 3)

    t = time.perf_counter()
    buffer = io.BytesIO()
    with pa.ipc.new_stream(buffer, sliced.schema) as writer:
        for batch in sliced.to_batches(max_chunksize=65_536):
            writer.write_batch(batch)
    result["slice_arrow_stream_mb"] = mb(buffer.tell())
    result["slice_arrow_write_s"] = round(time.perf_counter() - t, 3)
    return result


results = [
    measure("facility_1500x366x4", build(1500, 366, 4), date(2008, 1, 1), date(2009, 12, 31)),
    measure("national_2000x1040x10", build(2000, 1040, 10), date(2022, 1, 1), date(2023, 12, 31)),
]
print(json.dumps(results, indent=2))
