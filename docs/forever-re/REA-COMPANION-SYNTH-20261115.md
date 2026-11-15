# Retail synthesis reference: initial REA pass

2026-11-15, supplementary companion investigation for coordinator message
`fb745897f560f1c9`, task `3afe7d7093b6`. This pass avoids the client lead's
concurrent config-default, chat-window and background-filter investigation.
The preset-default/build 144 work remains with its existing owners; 143 was
already live according to the lead's current instructions. No release is gated
on this note.

## Observed artifact and entry addresses

Target: `[redacted-path] FANTASY XI/FFXiMain.dll`.
SHA256: `59b9c813ad46350e9b5681e4540a28463f55a33d4a5c938d62e50da95ed1ea67`.
Size: 2,901,584 bytes. REA reports PE, 32-bit, architecture `386`.
`full-1.2.2` is the directory label; this pass does not establish the client
version or an installed/running client's identity.

The following are **observed PE export-table entries**, obtained with the
installed REA CLI's `inspect-native-exports` operation. These are static image
addresses, not runtime process addresses. No function was called or decompiled.

| Export | Ordinal | Address |
| --- | --- | --- |
| `DllCanUnloadNow` | 1 | `0x1000f780` |
| `DllGetClassObject` | 2 | `0x1000f790` |
| `DllRegisterServer` | 3 | `0x1000f870` |
| `DllUnregisterServer` | 4 | `0x1000fbf0` |

Evidence: `ev_4022ddd4b98c3942527dd8feeabf02f7476c331ea53076836402960d2d2c3a36`;
provider `rea-native-exports`, version `1`, operation `inspect_native_exports`,
authority `shipped-artifact`, confidence `observed`. Coverage is complete over
the four export-directory entries, with no reported issues. This does not
establish call reachability, function behavior, runtime loading or safety.

## Decompilation: blocked before target analysis

The REA MCP call `binary_session({})` failed to start and explicitly instructed
the caller not to repeat startup. No MCP startup retry or registration repair
was attempted. The independent installed CLI fallback was then used.

The first CLI attempt lacked engine environment variables in the shell. The
existing configured Ghidra and Java paths were supplied to the child process
only, following REA's returned remedy:

- Ghidra: `[redacted-path]`.
- JDK: `[redacted-path]`.

REA then recognized Ghidra 12.1.4, its headless launcher and matching Linux/x64
native decompiler. Its Java probe failed with `runtime_unavailable` /
`ETIMEDOUT`: **"Command deadline elapsed during ownership preparation."**
`rea doctor --provider ghidra --json` reproduced that failure. A direct,
read-only Java version check succeeded in 0.611 seconds and reported Temurin
25.0.5, 64-bit. Thus the observed blocker is REA's owned-command preparation;
the generic suggestion to install/select a full JDK is not evidence that this
configured JDK is missing. No shared REA code, REA/client config or setup was
changed. No ownership check was bypassed.

Attempted query: `rea decompile <target> 0x101e15c0 --provider ghidra`, with a
caller-owned analysis snapshot path. It returned no pseudocode or successful
function Evidence. No Ghidra target session or game process was opened. The
successful portable export inspection finished normally.

## Pending synthesis questions

These addresses are **source-comment seeds, not confirmed retail findings**.
Their names come from `crates/vana-client/src/client/synth.rs` at client commit
`a31e144a21290663bd48f7423f5d24ecaa137334`.

| Question | Seeds | Status |
| --- | --- | --- |
| What does the menu request gate compare, and where is its timestamp written? | `FUN_101e15c0`, `FUN_101e15e0` | Unresolved: no successful REA decompile. The source comment's 23-second claim was not independently verified. |
| Which actor synthesis effect states and interrupt/result transitions does retail encode? | `FUN_1009f4c0` | Unresolved: not decompiled. Effect names, offsets, timings and completion-packet claims remain unverified here. |

The emulator is a separate reference:
`[redacted-path]`, commit
`faa18b85a4338cbf4d09843377d78797bcd979fa`.
`server/crates/xi-map/src/synth.rs:78` defines `SYNTH_TIME_MS = 16_000`, and
line 79 defines `SYNTH_COOLDOWN_MS = 15_000`. In
`session/synth.rs:135`, the request gate uses the cooldown; line 280 sets the
finish time from the duration minus a speed modifier. Those are observed
emulator-source facts, not proof of retail timing or the current live server.
They must not be equated with a client UI/menu timer.

Companion implication remains a design rule rather than a reverse-engineering
claim: animation can present synthesis progress, but only an authoritative
operation receipt may establish game outcome. This pass changes no animation,
clock, inventory, API key, authentication or native operation behavior.

## Retained evidence and next boundary

Full tool output is retained outside git under
`[redacted-path]`:

- `native-exports.json`: successful complete REA Evidence above.
- `provider-unavailable-before-env.json`: initial CLI environment failure.
- `FUN_101e15c0.json`: configured-provider failure; not a decompilation result.
- `doctor-ghidra.json`: matching ownership-preparation failure.
- `INVESTIGATION.json` and `VERIFICATION.json`: target/source identity, scope,
  checklist and evidence hashes.

The next decompile requires the REA owner to resolve the observed preparation
timeout and the MCP client to reconnect/restart if its in-app tools are needed.
Do not rerun setup, change shared configs, reinstall Java, weaken process
ownership checks or repeat the same failed decompile unchanged. No binaries or
bulk decompiled listings are committed. Runtime behavior, gameplay, rendering
parity and synthesis behavior remain untested by this pass.

## Handover addendum

The primary [REA findings](../REA-FINDINGS.md) record that this exact image is packed and its `.text` has no on-disk raw bytes. The export addresses above are declarations, not evidence that decoded function bodies are present at those addresses. The source-comment seeds were read from the documentation checkout at `a31e144a`, whose code is older than the active integration branch; never build from that checkout or treat those comments as a current retail decompile. Active client code lives on `private/int96-20261110`. Both the provider preflight failure and the packed-code boundary must be resolved before claiming retail synthesis behavior.
