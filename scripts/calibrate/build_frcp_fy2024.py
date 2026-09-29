#!/usr/bin/env python3
"""Regenerate calibration/frcp-fy2024.json from the published AO Table C-5
figures, recorded here as constants (not fetched over the network — the
point is a reproducible, offline "show your work" for the days-per-month
arithmetic in the source `method` notes, not a scraper).

Source: Table C-5 -- U.S. District Courts: Median Time Intervals From
Filing to Disposition of Civil Cases Terminated, by District and Method of
Disposition, During the 12-Month Period Ending September 30, 2024.
https://www.uscourts.gov/sites/default/files/2024-12/jb_c5_0930.2024.pdf
("Total" row, retrieved 2026-09-28.)

Run: python3 scripts/calibrate/build_frcp_fy2024.py
Writes: calibration/frcp-fy2024.json (pretty-printed, matching the
committed file byte-for-byte if the constants below haven't changed).
"""
import json
import pathlib

DAYS_PER_MONTH = 365.25 / 12  # 30.4375

SOURCE = {
    "title": (
        "Table C-5 — U.S. District Courts: Median Time Intervals From Filing to "
        "Disposition of Civil Cases Terminated, by District and Method of Disposition, "
        "During the 12-Month Period Ending September 30, 2024"
    ),
    "url": "https://www.uscourts.gov/sites/default/files/2024-12/jb_c5_0930.2024.pdf",
    "vintageStart": "2023-10-01",
    "vintageEnd": "2024-09-30",
    "retrieved": "2026-09-28",
}

# ("Total" row) method -> (cases, median months)
NO_COURT_ACTION = (69436, 8.3)
DURING_OR_AFTER_PRETRIAL = (21861, 13.5)


def days(months: float) -> float:
    return round(months * DAYS_PER_MONTH, 2)


def entry(ref: str, months: float, n: int, note: str) -> dict:
    return {
        "target": "edge-duration",
        "ref": ref,
        "distribution": {"mode": days(months)},
        "source": SOURCE,
        "n": n,
        "method": note,
    }


def main() -> None:
    n_no_action, months_no_action = NO_COURT_ACTION
    n_pretrial, months_pretrial = DURING_OR_AFTER_PRETRIAL
    doc = {
        "id": "frcp-fy2024",
        "title": "District court time-to-disposition by method, 12 months ending 9/30/2024",
        "description": (
            "Expected elapsed time (duration) for two `frcp-civil-procedure` pack edges, "
            "from the Administrative Office of the U.S. Courts' median time-to-disposition "
            "table (Table C-5), which breaks civil case terminations down by method of "
            "disposition. Only a median is published (no min/max), so each entry sets "
            "`distribution.mode` alone and leaves `min`/`max` unset — the engine's "
            "triangular duration collapses to the median when only `mode` is given. Months "
            "are converted to days at 365.25/12 = 30.4375 days/month."
        ),
        "entries": [
            entry(
                "frcp-civil-procedure::service-completed->voluntary-dismissal#0",
                months_no_action,
                n_no_action,
                f"'Total' row, 'No Court Action' column (a voluntary/stipulated dismissal or "
                f"settlement without a judicial ruling): {n_no_action:,} cases, median "
                f"{months_no_action} months. {months_no_action} * {DAYS_PER_MONTH:.4f} = "
                f"{days(months_no_action)} days.",
            ),
            entry(
                "frcp-civil-procedure::discovery-open->voluntary-dismissal#0",
                months_no_action,
                n_no_action,
                "Same 'No Court Action' figure as the other voluntary-dismissal edge above "
                "— the AO table does not distinguish which procedural stage the "
                "dismissal was reached at, only that no court ruling was involved.",
            ),
            entry(
                "frcp-civil-procedure::sj-ruling->summary-judgment-granted#0",
                months_pretrial,
                n_pretrial,
                f"'Total' row, 'During or After Pretrial' column (the closest published proxy "
                f"for a summary-judgment ruling, which is typically decided at or after the "
                f"close of pretrial/discovery): {n_pretrial:,} cases, median {months_pretrial} "
                f"months. {months_pretrial} * {DAYS_PER_MONTH:.4f} = {days(months_pretrial)} days.",
            ),
        ],
    }
    out = pathlib.Path(__file__).resolve().parents[2] / "calibration" / "frcp-fy2024.json"
    out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
