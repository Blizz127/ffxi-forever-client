# Third-party data

The item identifiers, labels, stack sizes and auction categories in `dist/data/recipes.json` are derived from the public LandSandBoat `sql/item_basic.sql` reference distributed under GNU GPL version 3. Credit to the DarkStar and LandSandBoat contributors. The exporter transforms identifier names into readable labels and selects only items referenced by the recipe catalog and its eight crystals.

Upstream project: https://github.com/LandSandBoat/server

The input file SHA-256 and the catalog TSV source identities are embedded in `recipes.json`. `tools/export-recipes.py` provides the transformation source. The original GPLv3 text is included at `LICENSES/LandSandBoat-GPL-3.0.txt`.

FFXI and FFXIV are Square Enix trademarks. The original companion interface is inspired by crafting-log organization and does not contain extracted artwork or UI assets.
