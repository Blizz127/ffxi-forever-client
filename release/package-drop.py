#!/usr/bin/env python3
"""Package recorded flavor builds locally; never upload or change a channel."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


# AMD FidelityFX Super Resolution 1.0 (MIT), ported to WGSL in render/fsr.rs for the Forever upscaler option.
# The copyright and MIT text are AMD's own, from ffx_fsr1.h in the FidelityFX FSR 2.2 source (downloaded 2026-11-08 with
# the owner's approval, kept outside this repository).
# The copyright line is that of ffx_fsr1.h in AMD's FidelityFX-FSR2 2.2.1 source (src/ffx-fsr2-api/shaders), checked 2026-11-08.
FSR2_NOTICE = """
Forever upscaler: AMD FidelityFX Super Resolution 2.2.1 (temporal), ported to WGSL (render/fsr2.wgsl, render/fsr2.rs).
Copyright (c) 2022-2023 Advanced Micro Devices, Inc. All rights reserved.
MIT License (the same terms as above): permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction, subject to the above copyright notice and
this permission notice being included in all copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY.
"""

FSR_NOTICE = """
Forever upscaler: AMD FidelityFX Super Resolution 1.0 (EASU and RCAS), ported to WGSL (render/fsr.rs).
Copyright (c) 2022 Advanced Micro Devices, Inc. All rights reserved.
MIT License. Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to
whom the Software is furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all copies or substantial portions of the
Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("flavor", choices=("classic", "forever"))
    parser.add_argument("version")
    parser.add_argument("output", type=Path, help="New drop directory; must not exist")
    parser.add_argument("--source-ref", default="HEAD", help="Local source candidate with the identical tree")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent

    def git(*cmd):
        return subprocess.check_output(["git", "-C", str(repo), *cmd], text=True).strip()

    product, suffix = ("vanaclient", "vana") if args.flavor == "classic" else ("forever-client", "forever")
    match = re.fullmatch(r"\d+\.\d+\.\d+-" + suffix + r"\.(\d+)", args.version)
    if not match or git("status", "--porcelain"):
        parser.error("version must match flavor, and the build source must be committed")
    if args.flavor == "forever":
        auth = (repo / "crates/vana-client/src/net/auth.rs").read_text()
        build = re.search(r"pub const FOREVER_BUILD: u32 = (\d+);", auth).group(1)
        if match.group(1) != build:
            parser.error("Forever version does not match FOREVER_BUILD")
    commit, tree = git("rev-parse", "HEAD"), git("rev-parse", "HEAD^{tree}")
    source = git("rev-parse", args.source_ref + "^{commit}")
    if git("rev-parse", source + "^{tree}") != tree:
        parser.error("source candidate differs from the build tree")
    binaries = {}
    for platform, tail in (("linux", "linux-x86_64"), ("windows", "windows-x86_64.exe")):
        binary = repo / "target/assets" / args.flavor / ("vana-client-" + tail)
        record = json.loads(binary.with_suffix(binary.suffix + ".build.json").read_text())
        expected = dict(flavor=args.flavor, platform=platform, build_commit=commit, source_tree=tree, binary_sha256=sha(binary))
        if record != expected:
            parser.error(f"{platform} artifact does not match this source and flavor")
        binaries[platform] = binary
    # Validate everything before creating the immutable drop directory.
    args.output.mkdir(parents=True, exist_ok=False)
    notice = ("This is a fork of vana-client (https://github.com/lenonk/vana-client) by Lenon Kitchens, MIT License.\n"
              f"Version {args.version}; flavor {args.flavor}. Source revision: {source}.\n"
              "Fork repository: https://github.com/Blizz127/vana-client.\n"
              "No game data is included; the client reads the player's own FINAL FANTASY XI install.\n"
              "FINAL FANTASY is a registered trademark of Square Enix Holdings Co., Ltd. This is an unofficial fan project.\n"
              "Bundled fonts retain their licenses (SIL Open Font License); see the source repository.\n")
    if args.flavor == "forever":
        notice += FSR_NOTICE + FSR2_NOTICE
    drop = dict(version=args.version, flavor=args.flavor, commit=source, build_commit=commit, source_tree=tree)
    with tempfile.TemporaryDirectory() as tmp:
        for platform, binary in binaries.items():
            stage = Path(tmp) / platform
            stage.mkdir()
            exe = "vana-client.exe" if platform == "windows" else "vana-client"
            shutil.copyfile(binary, stage / exe)
            (stage / exe).chmod(0o755)
            shutil.copyfile(repo / "LICENSE", stage / ("LICENSE.txt" if platform == "windows" else "LICENSE"))
            (stage / ("NOTICE.txt" if platform == "windows" else "NOTICE")).write_text(notice)
            if args.flavor == "classic":
                shutil.copyfile(repo / "release/notes/vanaclient-README.txt", stage / "README.txt")
            if platform == "linux":
                for name in ("vana-client.desktop", "vana-client.png", "vana-client-512.png"):
                    shutil.copyfile(repo / "assets" / name, stage / name)
                name = f"{product}-{args.version}-linux-x86_64.tar.zst"
                tar = Path(tmp) / "linux.tar"

                def owner(info):
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    return info

                with tarfile.open(tar, "w") as archive:
                    for path in sorted(stage.iterdir()):
                        archive.add(path, arcname=path.name, filter=owner)
                subprocess.run(["zstd", "-q", "-19", str(tar), "-o", str(args.output / name)], check=True)
            else:
                name = f"{product}-{args.version}-windows-x86_64.zip"
                with zipfile.ZipFile(args.output / name, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
                    for path in sorted(stage.iterdir()):
                        archive.write(path, path.name)
            drop[platform] = dict(file=name, sha256=sha(args.output / name), exe=exe, binary_sha256=sha(binary))
            if platform == "windows":
                drop[platform]["signed"] = False
    (args.output / "SHA256SUMS").write_text("".join(f"{drop[p]['sha256']}  {drop[p]['file']}\n" for p in binaries))
    (args.output / "DROP.json").write_text(json.dumps(drop, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
