# REA findings on the retail client (newest first)

Notes only: addresses, sizes, hashes and what was read. No game binary and no decompiled listing is kept here.

## 2026-11-15: FFXiMain.dll is packed; REA could not start; no question answered yet

Task: coordinator 3afe7d7093b6 ("all forever projects at minimum need REA used"), first pass by sw-fb. Questions waiting:
(1) the config-tool options' built-in defaults and the reader at start-up, (2) the log window's rule (config 0x0f / 0x10 /
0x11), (3) how a 3D target of another size reaches the screen. None can be answered yet; this is why.

### The file

| item | value |
|---|---|
| path (read-only) | [redacted-path] FANTASY XI/FFXiMain.dll |
| size | 2,901,584 bytes |
| sha256 | 59b9c813ad46350e9b5681e4540a28463f55a33d4a5c938d62e50da95ed1ea67 |
| PE | PE32 (optional header magic 0x10b), 7 sections |
| image base | 0x10000000 |
| entry point | RVA 0xbb1a50 (VA 0x10bb1a50) |

### Section table (from the PE section headers)

| section | RVA | virtual size | raw size on disk | Shannon entropy of the raw bytes | characteristics |
|---|---|---|---|---|---|
| .text | 0x1000 | 0x32762e (3.3 MB) | **0** | 0.00 (no bytes) | 0xe0000080 |
| .rdata | 0x329000 | 0x25595 | 0x26000 | 6.19 | 0x40000040 |
| .data | 0x34f000 | 0x677b49 | 0x85000 | 4.39 | 0xc0000040 |
| .data1 | 0x9c7000 | 0x8e0 | 0x1000 | 2.52 | 0xc0000040 |
| .rsrc | 0x9c8000 | 0x3140 | 0x3200 | 4.29 | 0x40000040 |
| POL1 | 0x9cc000 | 0x1e5b38 (1.9 MB) | 0x1e5c00 | **7.50** | 0xe0000040 |
| .reloc | 0xbb2000 | 0x2f274 | 0x2f400 | 6.72 | 0x42000040 |

### What it says

- Observed: `.text` has no bytes in the file (raw size 0; its flags carry the "uninitialised data" bit 0x80 together with
  execute / read / write). The program's code only exists in memory after the loader has run.
- Observed: the entry point (RVA 0xbb1a50) lies inside `POL1` (RVA 0x9cc000 to 0xbb1b38), a 1.9 MB section with entropy 7.50
  that is executable and writable. High entropy does not tell compression from encryption.
- Inference, from the section name only: `POL1` is the PlayOnline protector's stub and packed payload. Nothing here proves it.
- Consequence: the file on disk contains no readable code. The `FUN_xxxxxxxx` names in our source (for example
  `FUN_10016c30` in config.rs's comment on the config tool's values: VA 0x10016c30 is inside `.text`) are addresses in the
  UNPACKED, loaded image, so they came from an unpacked image or a listing of one, not from this file. Two source comments
  name such a listing, "all.asm": `client/net.rs:104` ("all.asm 167839") and `client/camera.rs:660` ("all.asm
  1001fefc..1001ff3f"). Where that listing is kept is not recorded in this repository (not found under
  [redacted-path] to depth 3; not searched further).
- So nothing about the built-in defaults of the config-tool options, the log rule's defaults or the 3D target's filtering
  can be read from this file statically, with REA or any other tool. `TOOL_DEFAULTS_RECORD` (config.rs) keeps its
  "source not recorded" rows; this note changes none of them.

### How it was read

A small Python 3 script run read-only (`python3 -I`), parsing the PE headers and computing a Shannon entropy per section
over the raw bytes. Nothing was executed, unpacked or loaded; the REA server was NOT used (see below).

### The REA server state

- The `rea` MCP tools are listed and load (`open_binary`, `binary_overview`, `inspect_pe_imports`, `inspect_byte_entropy`,
  `search_strings`, `procedure_pseudo_code`, `xrefs`, `close_binary` and the rest), after the skill `reverse-engineer-anything`
  was invoked.
- The first call to each of `inspect_byte_entropy`, `inspect_pe_imports` and `binary_session` returned the same error, verbatim:
  "Do not repeat this startup. Tell the user REA could not start and ask them to restart the MCP client, then run `rea doctor`."
  Per the skill the start was retried once through a second tool and then not repeated.
- Relayed by the lead (not run here): the read-only `rea doctor` reports the claude and codex registrations as stale
  ("run rea setup, then restart the client"). The task forbids `rea setup`, so it was not run, and neither was `rea doctor`
  in this lane. No REA or client config was edited.

### Open ways forward (the lead's, reported to the coordinator)

1. Someone supplies an unpacked image or listing (its path and sha256 recorded here), which REA can then open.
2. A static unpack of the `POL1` stub, once REA works.
3. No debugger dump or run of the program by this lane.
