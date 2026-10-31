#!/usr/bin/env python3
"""Build an extension separately from the host and package its native library."""

import argparse
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tomllib
import zipfile


def package(args):
    crate = Path(args.crate).resolve()
    cargo_manifest = crate / "Cargo.toml"
    manifest = tomllib.loads((crate / "extension.toml").read_text())
    target_args = ["--target", args.target] if args.target else []
    cfg = subprocess.check_output(["rustc", "--print", "cfg", *target_args], text=True)
    values = dict(line.split("=", 1) for line in cfg.splitlines() if "=" in line)
    platform = f"{json.loads(values['target_os'])}-{json.loads(values['target_arch'])}"
    member = manifest["targets"][platform]
    if "\\" in member or not member or PurePosixPath(member).is_absolute() or any(
        part in (".", "..") for part in member.split("/")
    ):
        raise ValueError("target library must have an enclosed, relative archive path")
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--no-deps", "--format-version", "1",
        "--manifest-path", str(cargo_manifest),
    ], text=True))
    selected = next(p for p in metadata["packages"] if Path(p["manifest_path"]).resolve() == cargo_manifest)
    command = ["cargo", "build", "--manifest-path", str(cargo_manifest), "--package", selected["name"], "--message-format=json-render-diagnostics", *target_args]
    if not args.debug:
        command.append("--release")
    process = subprocess.Popen(command, stdout=subprocess.PIPE, text=True)
    libraries = []
    for line in process.stdout:
        message = json.loads(line)
        if message.get("reason") == "compiler-message":
            rendered = message["message"].get("rendered")
            if rendered:
                print(rendered, file=sys.stderr, end="")
        if message.get("reason") == "compiler-artifact" and message["package_id"] == selected["id"]:
            libraries.extend(Path(p) for p in message["filenames"] if Path(p).suffix in (".so", ".dylib", ".dll"))
    if process.wait():
        raise RuntimeError("extension build failed")
    if len(libraries) != 1:
        raise ValueError("extension must build exactly one native cdylib")
    # A single-target package advertises only the library actually included.
    package_info = manifest["package"]
    archive_manifest = "\n".join([
        "[package]", f"name = {json.dumps(package_info['name'])}",
        f"version = {json.dumps(package_info['version'])}", "", "[targets]",
        f"{json.dumps(platform)} = {json.dumps(member)}", "",
    ])
    output = Path(args.output) if args.output else Path("extensions") / f"{selected['name']}.zip"
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("extension.toml", archive_manifest)
        archive.write(libraries[0], member)
    print(output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("crate", help="extension crate directory containing Cargo.toml and extension.toml")
    parser.add_argument("--debug", action="store_true", help="package a debug build")
    parser.add_argument("--target", help="Rust target triple for cross compilation")
    parser.add_argument("--output", help="output ZIP path (default: extensions/<crate>.zip)")
    try:
        package(parser.parse_args())
    except (OSError, ValueError, KeyError, StopIteration, subprocess.CalledProcessError, RuntimeError) as error:
        parser.exit(1, f"cannot package extension: {error}\n")
