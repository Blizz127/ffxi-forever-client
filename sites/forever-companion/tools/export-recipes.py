#!/usr/bin/env python3
"""Export public FFXI recipe metadata without reading game DATs or player data.

Example:
  python3 tools/export-recipes.py --client-source ../../crates/vana-client/src/client \
    --item-sql /path/to/lsb/sql/item_basic.sql --output dist/data/recipes.json

This parses INSERT values as data; it never executes SQL. Humanized item names are
derived from the public SQL identifiers, rather than the game's localized DATs.
"""
import argparse
import collections
import csv
import hashlib
import json
import re
from pathlib import Path


CRAFTS = dict(zip(
    ("wood", "smith", "gold", "cloth", "leather", "bone", "alchemy", "cook"),
    ("Woodworking", "Smithing", "Goldsmithing", "Clothcraft", "Leathercraft", "Bonecraft", "Alchemy", "Cooking"),
))


def sql_values(text):
    """Split a SQL VALUES tuple without splitting commas in quoted strings."""
    values, field, quoted, escaped = [], [], False, False
    for char in text:
        if escaped:
            field.append(char)
            escaped = False
        elif char == "\\" and quoted:
            escaped = True
        elif char == "'":
            quoted = not quoted
        elif char == "," and not quoted:
            values.append("".join(field).strip())
            field = []
        else:
            field.append(char)
    if quoted or escaped:
        raise ValueError("Malformed SQL tuple")
    values.append("".join(field).strip())
    return values


def humanize(name):
    # Remove units used by the SQL's descriptive name, preserving the full source
    # identifier alongside the display name. Sort names often abbreviate words.
    name = re.sub(r"^(?:piece|pieces|ingot|sheet|spool|ball|bolt|square|strip|chunk|handful|pinch|bag|sack|bottle|flask|vial|jar|jug|pot|bowl|plate|serving|slice|loaf|loaves|cup|can|tin|lump|clump|bundle|pair|set|block|stick|sprig|head|ear|bunch|cluster|bale|tuft|skein|length|roll|strand|slab|brick)_of_", "", name)
    return name.replace("_", " ").title()


def fingerprint(path):
    return {"file": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client-source", required=True, type=Path)
    parser.add_argument("--item-sql", required=True, type=Path)
    parser.add_argument("--output", type=Path, default=Path("dist/data/recipes.json"))
    args = parser.parse_args()
    recipe_path = args.client_source / "synth_era_public.tsv"
    era_path = args.client_source / "item_era_public.tsv"
    era = {}
    with era_path.open() as stream:
        for row in csv.DictReader(stream, delimiter="\t"):
            era[int(row["itemid"])] = {"era": row["era"], "level": int(row["level"]), "stage0": row["stage0"]}
    all_items = {}
    insert = re.compile(r"^INSERT INTO `item_basic` VALUES \((.*)\);(?:\s*--.*)?$")
    for line in args.item_sql.read_text().splitlines():
        match = insert.match(line)
        if not match:
            continue
        fields = sql_values(match.group(1))
        if len(fields) != 10:
            raise ValueError(f"Unexpected item_basic tuple columns: {len(fields)}")
        item_id = int(fields[0])
        all_items[item_id] = {
            "id": item_id, "name": humanize(fields[2]), "sourceName": fields[2],
            "stackSize": int(fields[6]), "auctionCategory": fields[8].lstrip("@"),
            "auctionable": fields[8] not in ("@NONE", "0") and "@FLAG_NOAUCTION" not in fields[7],
            **era.get(item_id, {"era": None, "level": None, "stage0": None}),
        }
    recipes, referenced = [], set(range(4096, 4104))
    with recipe_path.open() as stream:
        for row in csv.DictReader(stream, delimiter="\t"):
            craft = row["main_craft"]
            skills = [{"craft": craft, "level": int(row["main_level"])}]
            for sub in filter(None, row["subcrafts"].split(";")):
                subcraft, level = sub.split(":")
                skills.append({"craft": subcraft, "level": int(level)})
            ingredients = collections.Counter(map(int, row["ingredients"].split(";")))
            result_id, crystal = int(row["result"]), int(row["crystal"])
            hq = [{"itemId": int(row[f"hq{i}"]), "quantity": int(row[f"hq{i}_qty"])} for i in range(1, 4)]
            result_era = era.get(result_id)
            # Base-era result visibility and cap are copied from the client's
            # crafting_log::visible + era_items::allowed_core. Runtime character
            # skills, keys, inventory and status still require server validation.
            base_eligible = None if result_era is None else (
                result_era["era"] == "BASE" and result_era["level"] <= 60
                and all(skill["level"] <= 70 for skill in skills)
            )
            recipes.append({
                "id": int(row["recipe_id"]), "craft": craft, "level": int(row["main_level"]),
                "skills": skills, "keyItem": int(row["key_item"]), "crystal": crystal,
                "hqCrystal": int(row["hq_crystal"]),
                "ingredients": [{"itemId": item_id, "quantity": quantity} for item_id, quantity in ingredients.items()],
                "result": {"itemId": result_id, "quantity": int(row["result_qty"])},
                "hq": hq, "baseEligible": base_eligible,
            })
            referenced.update(ingredients)
            referenced.update((crystal, result_id))
            referenced.update(item["itemId"] for item in hq if item["itemId"])
            if int(row["hq_crystal"]):
                referenced.add(int(row["hq_crystal"]))
    missing = sorted(referenced - all_items.keys())
    items = {str(item_id): all_items.get(item_id, {
        "id": item_id, "name": f"Item {item_id}", "sourceName": None,
        "stackSize": 1, "auctionCategory": None, "auctionable": False,
        **era.get(item_id, {"era": None, "level": None, "stage0": None}),
    }) for item_id in sorted(referenced)}
    provenance = [fingerprint(recipe_path), fingerprint(era_path), fingerprint(args.item_sql)]
    for name in ("synth_era_public.source.json", "item_era_public.source.json"):
        provenance.append({**fingerprint(args.client_source / name), "upstream": json.loads((args.client_source / name).read_text())})
    data = {
        "meta": {
            "schemaVersion": 1, "count": len(recipes), "itemCount": len(items),
            "nameCoverage": len(items) - len(missing), "missingNames": missing,
            "baseEligibleCount": sum(r["baseEligible"] is True for r in recipes),
            "provenance": provenance,
            "names": "Humanized public item_basic.sql identifiers; no game DATs or icons copied.",
            "eligibility": "baseEligible checks result BASE era + result level <=60 + every craft requirement <=70. Character skills, key items, inventory, status and active server era must still be validated by the server. Null indicates unknown metadata.",
        },
        "crafts": [{"id": key, "name": name} for key, name in CRAFTS.items()],
        "items": items, "recipes": recipes,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(json.dumps({"recipes": len(recipes), "items": len(items), "baseEligible": data["meta"]["baseEligibleCount"], "missingNames": missing, "output": str(args.output)}))


if __name__ == "__main__":
    main()
