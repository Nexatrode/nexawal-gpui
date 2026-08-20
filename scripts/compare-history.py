#!/usr/bin/env python3
"""Compare a Feather history CSV with a NexaWal/GPUI history CSV.

The two wallets intentionally export different column names. This tool reduces
both formats to a transaction-id keyed representation and compares the fields
that should be implementation-independent. It never reads wallet keys or cache
files.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any

PICONERO_PER_XMR = Decimal("1000000000000")
FIELDS = ("direction", "amount_piconero", "fee_piconero", "height")


def xmr_to_piconero(value: str | None) -> int | None:
    if value is None or not value.strip() or value.strip() == "?":
        return None
    try:
        amount = Decimal(value.strip()) * PICONERO_PER_XMR
        if amount != amount.to_integral_value():
            raise ValueError(f"not an exact piconero amount: {value!r}")
        return int(amount)
    except (InvalidOperation, ValueError) as exc:
        raise ValueError(f"invalid XMR amount {value!r}") from exc


def integer(value: str | None) -> int | None:
    if value is None or not value.strip() or value.strip() == "?":
        return None
    return int(value.strip())


def direction(value: str | None) -> str:
    normalized = (value or "").strip().lower()
    if normalized in {"in", "inbound", "received", "receive"}:
        return "in"
    if normalized in {"out", "outbound", "sent", "send"}:
        return "out"
    return normalized


def read_audit_rows(path: Path, audit_node: str | None) -> dict[str, dict[str, Any]]:
    document = json.loads(path.read_text(encoding="utf-8"))
    targets = document.get("targets")
    if not isinstance(targets, list):
        raise ValueError(f"{path} is not a sync-audit report")
    if audit_node:
        targets = [
            target
            for target in targets
            if target.get("node") == audit_node or target.get("node_label") == audit_node
        ]
    if len(targets) != 1:
        raise ValueError(
            f"{path} contains {len(targets)} matching audit targets; use --audit-node"
        )
    rows = targets[0].get("transfers")
    if not isinstance(rows, list):
        raise ValueError(f"{path} audit target has no transfer list")
    result: dict[str, dict[str, Any]] = {}
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError(f"{path} contains a malformed transfer row")
        txid = str(row.get("txid", "")).strip().lower()
        if not txid:
            raise ValueError(f"{path} contains a transfer with no transaction ID")
        if txid in result:
            raise ValueError(f"{path}: duplicate transaction ID {txid}")
        result[txid] = {
            "txid": txid,
            "direction": direction(str(row.get("direction", ""))),
            "amount_piconero": row.get("amount_piconero"),
            "fee_piconero": row.get("fee_piconero"),
            "height": row.get("height"),
        }
    return result


def read_rows(path: Path, audit_node: str | None = None) -> dict[str, dict[str, Any]]:
    if path.suffix.lower() == ".json":
        return read_audit_rows(path, audit_node)
    with path.open(newline="", encoding="utf-8-sig") as stream:
        rows = list(csv.DictReader(stream))
    if not rows:
        raise ValueError(f"{path} has no data rows")
    headers = set(rows[0])
    is_feather = {"blockHeight", "balanceDelta", "txid"}.issubset(headers)
    is_gpui = {"amount_piconero", "block", "txid"}.issubset(headers)
    if not is_feather and not is_gpui:
        raise ValueError(
            f"{path} is neither a Feather export nor a NexaWal export; headers: {sorted(headers)}"
        )

    result: dict[str, dict[str, Any]] = {}
    for row_number, row in enumerate(rows, start=2):
        txid = (row.get("txid") or "").strip().lower()
        if not txid:
            raise ValueError(f"{path}:{row_number}: empty transaction ID")
        if txid in result:
            raise ValueError(f"{path}:{row_number}: duplicate transaction ID {txid}")
        if is_feather:
            normalized_direction = direction(row.get("direction"))
            amount = xmr_to_piconero(row.get("amount"))
            balance_delta = xmr_to_piconero(row.get("balanceDelta"))
            # Feather's outgoing `amount` is the payment alone, while
            # WalletCore's outgoing amount is the wallet debit (payment + fee).
            # `balanceDelta` is Feather's equivalent gross debit.
            if normalized_direction == "out" and balance_delta is not None:
                amount = abs(balance_delta)
            normalized = {
                "txid": txid,
                "direction": normalized_direction,
                "amount_piconero": amount,
                "fee_piconero": xmr_to_piconero(row.get("fee")),
                "height": integer(row.get("blockHeight")),
            }
        else:
            normalized = {
                "txid": txid,
                "direction": direction(row.get("direction")),
                "amount_piconero": integer(row.get("amount_piconero"))
                if row.get("amount_piconero")
                else xmr_to_piconero(row.get("amount_xmr")),
                "fee_piconero": integer(row.get("fee_piconero"))
                if row.get("fee_piconero")
                else xmr_to_piconero(row.get("fee_xmr")),
                "height": integer(row.get("block")),
            }
        result[txid] = normalized
    return result


def apply_height(rows: dict[str, dict[str, Any]], maximum: int | None) -> dict[str, dict[str, Any]]:
    if maximum is None:
        return rows
    return {
        txid: row
        for txid, row in rows.items()
        if row["height"] is None or row["height"] <= maximum
    }


def compare(
    left: dict[str, dict[str, Any]],
    right: dict[str, dict[str, Any]],
    allow_missing_fees: bool = False,
) -> dict[str, Any]:
    left_ids = set(left)
    right_ids = set(right)
    missing_from_right = sorted(left_ids - right_ids)
    missing_from_left = sorted(right_ids - left_ids)
    mismatches = []
    unavailable_fields = []
    for txid in sorted(left_ids & right_ids):
        different = []
        for field in FIELDS:
            left_value = left[txid][field]
            right_value = right[txid][field]
            # Feather is the left-hand source of truth. Fresh WalletCore audit
            # reports must contain every fee Feather provides. The override is
            # only for comparing reports made before historical fee support.
            if field == "fee_piconero" and left_value is not None and right_value is None:
                if allow_missing_fees:
                    unavailable_fields.append({"txid": txid, "field": field})
                    continue
            if left_value != right_value:
                different.append(field)
        if different:
            mismatches.append(
                {
                    "txid": txid,
                    "fields": different,
                    "left": left[txid],
                    "right": right[txid],
                }
            )
    return {
        "left_count": len(left),
        "right_count": len(right),
        "missing_from_right": missing_from_right,
        "missing_from_left": missing_from_left,
        "mismatches": mismatches,
        "unavailable_fields": unavailable_fields,
        "pass": not missing_from_right and not mismatches,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("feather_csv", type=Path)
    parser.add_argument("gpui_csv", type=Path)
    parser.add_argument(
        "--max-height",
        type=int,
        help="only compare rows at or below this height (useful for an older Feather export)",
    )
    parser.add_argument(
        "--allow-extra",
        action="store_true",
        help="allow transaction IDs present only in the GPUI export",
    )
    parser.add_argument(
        "--audit-node",
        help="select a node label or URL when the GPUI input is a sync_audit_*.json report",
    )
    parser.add_argument(
        "--allow-missing-fees",
        action="store_true",
        help="allow fees missing only from legacy WalletCore reports",
    )
    parser.add_argument("--json", action="store_true", help="emit the report as JSON")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        feather = apply_height(read_rows(args.feather_csv), args.max_height)
        gpui = apply_height(read_rows(args.gpui_csv, args.audit_node), args.max_height)
        report = compare(feather, gpui, allow_missing_fees=args.allow_missing_fees)
    except (OSError, ValueError, csv.Error) as exc:
        print(f"history comparison failed: {exc}", file=sys.stderr)
        return 2

    strict_pass = report["pass"] and (
        args.allow_extra or not report["missing_from_left"]
    )
    report.update(
        {
            "feather_csv": str(args.feather_csv),
            "gpui_csv": str(args.gpui_csv),
            "max_height": args.max_height,
            "allow_missing_fees": args.allow_missing_fees,
            "strict_pass": strict_pass,
        }
    )
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(
            f"Feather rows: {report['left_count']} · GPUI rows: {report['right_count']}"
        )
        print(f"Missing from GPUI: {len(report['missing_from_right'])}")
        print(f"Only in GPUI: {len(report['missing_from_left'])}")
        print(f"Field mismatches: {len(report['mismatches'])}")
        if report["unavailable_fields"]:
            print(f"Fields unavailable on one side: {len(report['unavailable_fields'])}")
        if report["missing_from_right"]:
            print("Missing transaction IDs:")
            print("  " + "\n  ".join(report["missing_from_right"][:50]))
        if report["mismatches"]:
            print("Mismatches:")
            for mismatch in report["mismatches"][:20]:
                print(f"  {mismatch['txid']}: {', '.join(mismatch['fields'])}")
        print("PASS" if strict_pass else "FAIL")
    return 0 if strict_pass else 1


if __name__ == "__main__":
    raise SystemExit(main())
