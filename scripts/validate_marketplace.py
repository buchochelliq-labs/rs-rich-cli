#!/usr/bin/env python3
"""Validate rs-rich marketplace listings and indexes.

The format is docs/design/marketplace.md; the schemas are
schemas/marketplace/listing.schema.json and index.schema.json.

    python3 scripts/validate_marketplace.py LISTING.json ...
    python3 scripts/validate_marketplace.py --expect-invalid BAD.json ...
    python3 scripts/validate_marketplace.py --index MARKETPLACE/index.json

Each listing is checked twice over: against the JSON Schema, and by the
rules a schema cannot state (versions unique and newest first, a crate's
artifact URL naming its crate and version, `ansi-output` declared exactly
when a WASM version has a `fence-ansi` capability, the official tier only for
this repository's packages). An index is checked against its schema, and
every listing it names must exist, hash to the SHA-256 it records, carry the
id it is filed under and be valid; a listing the index leaves out is an
error too. Minisign signatures are not checked here: the marketplace crate
verifies them (the standard library has no Ed25519).

Only the standard library is needed. The schemas are evaluated by a small
evaluator of the JSON Schema keywords they use, which refuses a schema that
uses any other keyword, so the two cannot drift apart silently. When the
`jsonschema` package is installed (`pip install jsonschema`), every document
is validated by it as well and the two must agree; `--jsonschema require`
makes it mandatory and `--jsonschema off` skips it.

Exit status: 0 when everything passed (or, with `--expect-invalid`, every
file was refused), 1 when something did not, 2 for a usage or setup error.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCHEMA_DIR = ROOT / "schemas" / "marketplace"
LISTING_SCHEMA = SCHEMA_DIR / "listing.schema.json"
INDEX_SCHEMA = SCHEMA_DIR / "index.schema.json"

# The organisation whose packages may be listed as `official`.
OFFICIAL_PREFIX = "https://github.com/buchochelliq-labs/"
# The largest listing or index file read, in bytes.
MAX_FILE_BYTES = 1 << 20


class SetupError(Exception):
    """A schema or environment problem, not a problem with a document."""


# ---------------------------------------------------------------------------
# A JSON Schema evaluator for the keywords the marketplace schemas use.
# ---------------------------------------------------------------------------

# Keywords that only annotate.
ANNOTATIONS = {"$schema", "$id", "title", "description"}
# Keywords the evaluator implements (draft 2020-12 semantics).
SUPPORTED = ANNOTATIONS | {
    "$defs", "$ref", "type", "const", "enum", "required", "properties",
    "additionalProperties", "propertyNames", "items", "contains", "minItems",
    "maxItems", "uniqueItems", "minLength", "maxLength", "pattern", "minimum",
    "maximum", "allOf", "not", "if", "then", "else",
}


def check_keywords(schema, where="#"):
    """Raise SetupError if `schema` uses a keyword the evaluator lacks."""
    if isinstance(schema, bool):
        return
    if not isinstance(schema, dict):
        raise SetupError(f"{where}: a schema must be an object or a boolean")
    for key, value in schema.items():
        if key not in SUPPORTED:
            raise SetupError(f"{where}: unsupported keyword {key!r}")
        if key in ("properties", "$defs"):
            for name, sub in value.items():
                check_keywords(sub, f"{where}/{key}/{name}")
        elif key == "allOf":
            for i, sub in enumerate(value):
                check_keywords(sub, f"{where}/allOf/{i}")
        elif key in ("additionalProperties", "propertyNames", "items",
                     "contains", "not", "if", "then", "else"):
            check_keywords(value, f"{where}/{key}")


def json_type(value):
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "integer" if value.is_integer() else "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    return "object"


def json_equal(a, b):
    """JSON equality: `true` is not `1`, `1.0` is `1`."""
    ta, tb = json_type(a), json_type(b)
    if {ta, tb} <= {"integer", "number"}:
        return a == b
    if ta != tb:
        return False
    if ta == "array":
        return len(a) == len(b) and all(json_equal(x, y) for x, y in zip(a, b))
    if ta == "object":
        return a.keys() == b.keys() and all(json_equal(a[k], b[k]) for k in a)
    return a == b


def path_join(path, key):
    if isinstance(key, int):
        return f"{path}[{key}]"
    return f"{path}.{key}" if path else key


class Evaluator:
    def __init__(self, root):
        check_keywords(root)
        self.root = root

    def resolve(self, ref):
        if not ref.startswith("#/"):
            raise SetupError(f"only local references are supported: {ref!r}")
        node = self.root
        for part in ref[2:].split("/"):
            node = node[part]
        return node

    def errors(self, value, schema=None, path=""):
        """Every way `value` fails `schema`, as `path: message` strings."""
        schema = self.root if schema is None else schema
        if schema is True:
            return []
        if schema is False:
            return [f"{path or '(document)'}: not allowed"]
        out = []
        at = path or "(document)"
        if "$ref" in schema:
            out += self.errors(value, self.resolve(schema["$ref"]), path)
        kind = json_type(value)
        if "type" in schema:
            wanted = schema["type"]
            wanted = wanted if isinstance(wanted, list) else [wanted]
            ok = kind in wanted or (kind == "integer" and "number" in wanted)
            if not ok:
                return out + [f"{at}: expected {' or '.join(wanted)}, got {kind}"]
        if "const" in schema and not json_equal(value, schema["const"]):
            out.append(f"{at}: must be {json.dumps(schema['const'])}")
        if "enum" in schema and not any(json_equal(value, e) for e in schema["enum"]):
            choices = ", ".join(json.dumps(e) for e in schema["enum"])
            out.append(f"{at}: {json.dumps(value)} is not one of {choices}")
        if kind == "string":
            if "minLength" in schema and len(value) < schema["minLength"]:
                out.append(f"{at}: shorter than {schema['minLength']} characters")
            if "maxLength" in schema and len(value) > schema["maxLength"]:
                out.append(f"{at}: longer than {schema['maxLength']} characters")
            if "pattern" in schema and not re.search(schema["pattern"], value):
                out.append(f"{at}: {json.dumps(value)} does not match {schema['pattern']}")
        if kind in ("integer", "number"):
            if "minimum" in schema and value < schema["minimum"]:
                out.append(f"{at}: less than {schema['minimum']}")
            if "maximum" in schema and value > schema["maximum"]:
                out.append(f"{at}: more than {schema['maximum']}")
        if kind == "array":
            if "minItems" in schema and len(value) < schema["minItems"]:
                out.append(f"{at}: fewer than {schema['minItems']} items")
            if "maxItems" in schema and len(value) > schema["maxItems"]:
                out.append(f"{at}: more than {schema['maxItems']} items")
            if schema.get("uniqueItems"):
                for i, item in enumerate(value):
                    if any(json_equal(item, earlier) for earlier in value[:i]):
                        out.append(f"{path_join(path, i)}: a duplicate")
            if "items" in schema:
                for i, item in enumerate(value):
                    out += self.errors(item, schema["items"], path_join(path, i))
            if "contains" in schema and not any(
                not self.errors(item, schema["contains"]) for item in value
            ):
                out.append(f"{at}: no item matches {json.dumps(schema['contains'])}")
        if kind == "object":
            for name in schema.get("required", []):
                if name not in value:
                    out.append(f"{at}: missing {name!r}")
            properties = schema.get("properties", {})
            for name, sub in value.items():
                if name in properties:
                    out += self.errors(sub, properties[name], path_join(path, name))
                elif "additionalProperties" in schema:
                    extra = schema["additionalProperties"]
                    if extra is False:
                        out.append(f"{path_join(path, name)}: unknown key")
                    else:
                        out += self.errors(sub, extra, path_join(path, name))
                if "propertyNames" in schema:
                    for error in self.errors(name, schema["propertyNames"]):
                        out.append(f"{path_join(path, name)}: invalid key ({error.split(': ', 1)[1]})")
        for sub in schema.get("allOf", []):
            out += self.errors(value, sub, path)
        if "not" in schema and not self.errors(value, schema["not"], path):
            out.append(f"{at}: must not match {json.dumps(schema['not'])}")
        if "if" in schema:
            branch = "then" if not self.errors(value, schema["if"], path) else "else"
            if branch in schema:
                out += self.errors(value, schema[branch], path)
        # One message per problem, in order.
        return list(dict.fromkeys(out))


# ---------------------------------------------------------------------------
# What a schema cannot say.
# ---------------------------------------------------------------------------

SEMVER = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$"
)


def semver_key(version):
    """A sort key with SemVer 2.0 precedence (build metadata ignored)."""
    match = SEMVER.match(version)
    if not match:
        raise ValueError(version)
    major, minor, patch, pre = match.groups()
    if pre is None:
        # A release sorts after every pre-release of it.
        pre_key = (1,)
    else:
        parts = []
        for part in pre.split("."):
            # Numeric identifiers sort before alphanumeric ones.
            parts.append((0, int(part), "") if part.isdigit() else (1, 0, part))
        pre_key = (0, tuple(parts))
    return (int(major), int(minor), int(patch), pre_key)


def semantic_errors(listing):
    """The rules beyond the schema. Assumes the listing passed the schema."""
    out = []
    kind = listing["kind"]
    versions = listing["versions"]
    seen = {}
    for i, entry in enumerate(versions):
        at = f"versions[{i}]"
        number = entry["version"]
        precedence = semver_key(number)
        if precedence in seen:
            out.append(f"{at}.version: {number} repeats versions[{seen[precedence]}]")
        else:
            seen[precedence] = i
        if i and precedence >= semver_key(versions[i - 1]["version"]):
            out.append(
                f"{at}.version: {number} is listed after {versions[i - 1]['version']}; "
                "list versions newest first"
            )
        pairs = [(c["kind"], c["name"]) for c in entry["capabilities"]]
        for j, pair in enumerate(pairs):
            if pair in pairs[:j]:
                out.append(f"{at}.capabilities[{j}]: {pair[0]} {pair[1]!r} is declared twice")
        if kind == "wasm-plugin":
            ansi = any(k == "fence-ansi" for k, _ in pairs)
            declared = "ansi-output" in entry["permissions"]
            if ansi and not declared:
                out.append(f"{at}.permissions: a fence-ansi capability needs 'ansi-output'")
            if declared and not ansi:
                out.append(f"{at}.permissions: 'ansi-output' without a fence-ansi capability")
        if kind == "crate":
            name = listing["crate"]["name"]
            expected = f"https://static.crates.io/crates/{name}/{name}-{number}.crate"
            if entry["artifact"]["url"] != expected:
                out.append(f"{at}.artifact.url: must be {expected}")
    if listing["tier"] == "official" and not listing["repository"].startswith(OFFICIAL_PREFIX):
        out.append(f"tier: 'official' is only for packages under {OFFICIAL_PREFIX}")
    return out


# ---------------------------------------------------------------------------
# Documents.
# ---------------------------------------------------------------------------


def load_json(path):
    """Parse `path`, refusing duplicate keys, non-finite numbers and big files."""
    data = Path(path).read_bytes()
    if len(data) > MAX_FILE_BYTES:
        raise ValueError(f"larger than {MAX_FILE_BYTES} bytes")

    def no_duplicates(pairs):
        obj = {}
        for key, value in pairs:
            if key in obj:
                raise ValueError(f"duplicate key {key!r}")
            obj[key] = value
        return obj

    def no_constant(name):
        raise ValueError(f"{name} is not JSON")

    return json.loads(
        data.decode("utf-8"),
        object_pairs_hook=no_duplicates,
        parse_constant=no_constant,
    )


class Validator:
    def __init__(self, jsonschema_mode="auto"):
        self.listing = Evaluator(load_json(LISTING_SCHEMA))
        self.index = Evaluator(load_json(INDEX_SCHEMA))
        self.third_party = {}
        if jsonschema_mode == "off":
            return
        try:
            import jsonschema
        except ImportError:
            if jsonschema_mode == "require":
                raise SetupError("--jsonschema require: the jsonschema package is not installed")
            return
        for name, evaluator in (("listing", self.listing), ("index", self.index)):
            cls = jsonschema.validators.validator_for(evaluator.root)
            cls.check_schema(evaluator.root)
            self.third_party[name] = cls(evaluator.root)

    def schema_errors(self, document, which):
        evaluator = self.listing if which == "listing" else self.index
        errors = evaluator.errors(document)
        other = self.third_party.get(which)
        if other is not None:
            theirs = sorted(other.iter_errors(document), key=lambda e: list(e.path))
            if bool(theirs) != bool(errors):
                detail = "; ".join(e.message for e in theirs[:3]) or "no errors"
                errors.append(
                    f"(validators disagree: this script found {len(errors)} errors, "
                    f"jsonschema {detail})"
                )
        return errors

    def check_listing(self, document):
        errors = self.schema_errors(document, "listing")
        if not errors:
            errors = semantic_errors(document)
        return errors

    def check_listing_file(self, path):
        try:
            document = load_json(path)
        except (OSError, ValueError) as error:
            return [f"cannot read: {error}"]
        return self.check_listing(document)

    def check_index_file(self, index_path):
        """Errors in the index and in every listing it names, each prefixed
        with the file it is in."""
        index_path = Path(index_path)
        root = index_path.parent
        try:
            index = load_json(index_path)
        except (OSError, ValueError) as error:
            return [f"{index_path}: cannot read: {error}"]
        errors = [f"{index_path}: {e}" for e in self.schema_errors(index, "index")]
        if errors:
            return errors
        if index["expires"] <= index["generated"]:
            errors.append(f"{index_path}: expires is not after generated")
        named = set()
        for listing_id, entry in sorted(index["listings"].items()):
            path = root / entry["path"]
            named.add(entry["path"])
            if Path(entry["path"]).stem != listing_id:
                errors.append(f"{index_path}: listings.{listing_id}: the path must be "
                              f"packages/{listing_id}.json")
            try:
                data = path.read_bytes()
            except OSError as error:
                errors.append(f"{path}: cannot read: {error}")
                continue
            digest = hashlib.sha256(data).hexdigest()
            if digest != entry["sha256"]:
                errors.append(f"{path}: SHA-256 is {digest}, but the index records {entry['sha256']}")
            problems = self.check_listing_file(path)
            errors += [f"{path}: {p}" for p in problems]
            if not problems:
                document = load_json(path)
                if document["id"] != listing_id:
                    errors.append(f"{path}: id is {document['id']!r}, filed as {listing_id!r}")
        packages = root / "packages"
        if packages.is_dir():
            for path in sorted(packages.glob("*.json")):
                relative = f"packages/{path.name}"
                if relative not in named:
                    errors.append(f"{path}: not in the index")
        return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("files", nargs="*", type=Path, help="listing files")
    parser.add_argument("--index", type=Path, action="append", default=[],
                        help="an index.json; checks it and every listing it names")
    parser.add_argument("--expect-invalid", action="store_true",
                        help="every listing given must be refused")
    parser.add_argument("--jsonschema", choices=["auto", "require", "off"], default="auto",
                        help="also validate with the jsonschema package (default: when installed)")
    args = parser.parse_args(argv)
    if not args.files and not args.index:
        parser.error("name at least one listing or --index")
    try:
        validator = Validator(args.jsonschema)
    except SetupError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    engines = "the built-in evaluator" + (" and jsonschema" if validator.third_party else "")
    failed = 0
    for path in args.files:
        errors = validator.check_listing_file(path)
        if args.expect_invalid:
            if errors:
                print(f"ok (refused): {path}: {errors[0]}")
            else:
                print(f"FAIL: {path}: valid, but expected to be refused")
                failed += 1
        elif errors:
            failed += 1
            for error in errors:
                print(f"FAIL: {path}: {error}")
        else:
            print(f"ok: {path}")
    for index in args.index:
        errors = validator.check_index_file(index)
        if errors:
            failed += 1
            for error in errors:
                print(f"FAIL: {error}")
        else:
            print(f"ok: {index} and its listings")
    total = len(args.files) + len(args.index)
    print(f"{total - failed} of {total} passed ({engines})")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
