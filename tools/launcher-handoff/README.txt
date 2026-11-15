Synthetic launcher handoff check
===============================

This is a client-boundary test, not a claim that a real launcher account/token
session has passed. It runs the unmodified packaged Linux executable with no
arguments, FFXI_CONFIG and VANA_LOGIN_FILE, matching NativeForeverClient.Start.
The invented login JSON matches NativeForeverSecrets.WriteLoginFile. The actual
launcher process and accounts service are not invoked.

run.sh BINARY SHA256 OWN_DAT_ROOT EMPTY_EVIDENCE_DIRECTORY

The devbox runner uses vana-sandbox:build, cached runtime libraries, a private
Xvfb, read-only user-owned game data, software Vulkan, CPUs 0-11 and Docker
--network none. Only loopback is available. No host X display, real config,
credential file, account, token, remote endpoint or owner device is used. A
temporary self-signed loopback certificate uses the client's normal first-use
pinning in each isolated fixture directory; trust verification is not disabled.

The exact packaged SHA256 is checked before starting. Each case launches through
the real default entry point and checks one-use handoff deletion. Saved-login
cases assert the actual TLS request: game credentials, loader 2.1.2, correct
Forever marker/build, distinct launch token, blank OTP and no social-token leak.
Other cases check that a regular-server selection omits Forever auth fields,
and feed-only/malformed/oversized files cause no automatic game login. The
opening movie is disabled in this synthetic config so it reaches login.

The loopback server only returns refusals; it cannot create accounts, mint
tokens, start a lobby or enter a game. A 12-second observation includes the
client's 10-second reconnect interval and checks no auth replay. Graceful
WM_DELETE_WINDOW exits are checked; terminating a failed synthetic child is
failure cleanup, not a passing shutdown test. No request bodies or fixture
secret values are printed; client logs are checked for fixture-secret leaks.

Scope limits:
- Feed-only tests check safe startup/no autologin, not successful feed retrieval.
  Existing friends_feed/social_message tests cover scope rotation and retries.
- A regular-server selection in the Forever executable tests its runtime gate,
  not a new classic-build run. Existing classic flavor checks remain separate.
- Real launcher mint/session, Windows GUI, owner device, live auth/lobby/zone,
  server rollout and publication remain untested by this harness.
- Build 25 is an explicit assertion. Change it deliberately for future drops.

Launcher implementation reference (read-only audit, commit 983b9ebc):
  NativeForeverClient.cs: no-argument process, config/login environment variables
  NativeForeverSecrets.cs: one-use login JSON and separate friends-token file
  NativeForeverConfig.cs: named-server loader_version 2.1.2
  FfxiForeverLaunchTokenTests.cs: synthetic mint success/refusal/malformed cases
  FfxiForeverNativeLaunchFlowTests.cs: fake accounts -> actual launch flow -> probe
Those tests belong to the launcher lead; this harness does not replace or rerun
them and does not establish that their current revision passed.
