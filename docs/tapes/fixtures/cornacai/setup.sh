#!/bin/sh
# Cornacai's tape workspace, in the directory the tape runs in: a small
# program for the fake agent to edit, its transcript, and an empty config.
# Usage: setup.sh REPO
set -e
repo="$1"
mkdir -p src config
printf 'fn main() {\n    println!("Hello");\n}\n' > src/main.rs
cp "$repo/docs/tapes/fixtures/cornacai/demo.jsonl" demo.jsonl
