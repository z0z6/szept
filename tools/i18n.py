#!/usr/bin/env python3
"""Szept's languages.

Every text people see is written in Polish in the code, inside t(…)
(N_(…) marks texts that are translated later, where they are shown).
The Polish text is the key: i18n/<code>.json maps it to the translation,
with {name} placeholders and $1 kept as they are. A missing translation
shows in Polish.

    python3 tools/i18n.py          checks every language, then writes the
                                   data for both apps
    python3 tools/i18n.py missing  lists what each language still lacks

It collects the texts from the code (t, tf, N_), from the two HTML pages,
and from tools/figures.py and tools/sounds.py, and writes:

    i18n/pl.json                               every key, for translators
    crates/desktop/ui/i18n-data.js             the windows' copy
    crates/desktop/src/i18n_data.rs            the desktop's Rust side
    android/app/src/main/res/raw/i18n_*.json   Android's copy
    android/app/src/main/java/org/komunikator/app/I18nData.kt
"""

import html.parser
import importlib.util
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
I18N = ROOT / "i18n"

# (code, name in that language, written right to left). Polish first: it is
# the source.
LANGUAGES = [
    ("pl", "Polski", False),
    ("en", "English", False),
    ("zh", "中文（简体）", False),
    ("hi", "हिन्दी", False),
    ("es", "Español", False),
    ("fr", "Français", False),
    ("ar", "العربية", True),
    ("pt", "Português", False),
    ("ru", "Русский", False),
    ("ja", "日本語", False),
    ("de", "Deutsch", False),
]

CODE = [
    *sorted((ROOT / "crates/desktop/ui").glob("*.js")),
    *sorted((ROOT / "crates/desktop/src").glob("*.rs")),
    *sorted((ROOT / "android/app/src/main/java/org/komunikator/app").glob("*.kt")),
]
GENERATED = {"i18n-data.js", "figures-data.js", "sounds-data.js", "i18n_data.rs", "I18nData.kt",
             "FiguresData.kt", "SoundsData.kt"}
CALL = re.compile(r'(?<![\w.])(?:i18n::|crate::i18n::)?(?:t|tf|N_)\(\s*"((?:[^"\\\n]|\\.)*)"')
PLACEHOLDER = re.compile(r"\{[A-Za-z]\w*\}|\$\d")


def unescape(text, kind):
    if kind == ".kt":
        text = text.replace("\\$", "$")
    return json.loads('"' + text.replace("\\'", "'") + '"')


class Texts(html.parser.HTMLParser):
    """Text nodes and the attributes people read, as the windows look them up."""

    ATTRIBUTES = {"aria-label", "title", "placeholder"}

    def __init__(self):
        super().__init__()
        self.found = []
        self.skip = 0

    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style"):
            self.skip += 1
        for name, value in attrs:
            if name in self.ATTRIBUTES and value:
                self.add(value)

    def handle_endtag(self, tag):
        if tag in ("script", "style"):
            self.skip -= 1

    def handle_data(self, data):
        if not self.skip:
            self.add(data)

    def add(self, text):
        text = " ".join(text.split())
        if re.search(r"[^\W\d_]{2}", text):
            self.found.append(text)


def load_module(path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def collect():
    keys = {}
    for path in CODE:
        if path.name in GENERATED:
            continue
        text = path.read_text(encoding="utf-8")
        for match in CALL.finditer(text):
            key = unescape(match.group(1), path.suffix)
            keys.setdefault(key, f"{path.relative_to(ROOT)}:{text.count(chr(10), 0, match.start()) + 1}")
    for page in ("index.html", "chat.html"):
        parser = Texts()
        parser.feed((ROOT / "crates/desktop/ui" / page).read_text(encoding="utf-8"))
        for key in parser.found:
            keys.setdefault(key, f"crates/desktop/ui/{page}")
    figures = load_module(ROOT / "tools/figures.py")
    for figure in figures.FIGURES:
        keys.setdefault(figure["name"], "tools/figures.py")
        keys.setdefault(figure["caption"], "tools/figures.py")
    sounds = load_module(ROOT / "tools/sounds.py")
    for _, name, about in sounds.CHOICES:
        keys.setdefault(name, "tools/sounds.py")
        keys.setdefault(about, "tools/sounds.py")
    # Brand names and the like read the same everywhere.
    for same in ("Szept", "OK", "GIF", "XNO", "Nano"):
        keys.pop(same, None)
    return keys


def load(code):
    path = I18N / f"{code}.json"
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def check(keys):
    problems = 0
    for code, _, _ in LANGUAGES[1:]:
        table = load(code)
        missing = [key for key in keys if key not in table]
        extra = [key for key in table if key not in keys]
        broken = [key for key in keys if key in table
                  and sorted(PLACEHOLDER.findall(key)) != sorted(PLACEHOLDER.findall(table[key]))]
        if missing or broken:
            problems += 1
        print(f"{code}: {len(table) - len(extra)}/{len(keys)}"
              + (f", {len(missing)} missing" if missing else "")
              + (f", {len(extra)} unused" if extra else "")
              + (f", {len(broken)} with wrong placeholders: {broken[:3]}" if broken else ""))
    return problems


def write(keys):
    I18N.mkdir(exist_ok=True)
    (I18N / "pl.json").write_text(
        json.dumps({key: key for key in keys}, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    tables = {}
    for code, _, _ in LANGUAGES[1:]:
        table = load(code)
        tables[code] = {key: table[key] for key in keys if key in table}
        # Keep each translation file tidy and in the order of the keys.
        (I18N / f"{code}.json").write_text(
            json.dumps(tables[code], ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

    languages = [{"code": code, "name": name, "rtl": rtl} for code, name, rtl in LANGUAGES]
    (ROOT / "crates/desktop/ui/i18n-data.js").write_text(
        "// Generated by tools/i18n.py from i18n/*.json. Do not edit.\n\"use strict\";\n"
        f"const I18N_LANGUAGES = {json.dumps(languages, ensure_ascii=False)};\n"
        f"const I18N = {json.dumps(tables, ensure_ascii=False, separators=(',', ':'))};\n",
        encoding="utf-8")

    rust = ["// Generated by tools/i18n.py. Do not edit.", "", "/// Language code and its translations as JSON.",
            "pub const TRANSLATIONS: &[(&str, &str)] = &["]
    for code, _, _ in LANGUAGES[1:]:
        rust.append(f'    ("{code}", include_str!("../../../i18n/{code}.json")),')
    rust.append("];")
    (ROOT / "crates/desktop/src/i18n_data.rs").write_text("\n".join(rust) + "\n", encoding="utf-8")

    raw = ROOT / "android/app/src/main/res/raw"
    raw.mkdir(parents=True, exist_ok=True)
    for code, table in tables.items():
        (raw / f"i18n_{code}.json").write_text(
            json.dumps(table, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    kotlin = [
        "// Generated by tools/i18n.py. Do not edit.",
        "package org.komunikator.app",
        "",
        "/** A language Szept speaks: its code, its own name, and whether it is written right to left. */",
        "data class Language(val code: String, val name: String, val rtl: Boolean)",
        "",
        "val LANGUAGES: List<Language> = listOf(",
    ]
    for code, name, rtl in LANGUAGES:
        kotlin.append(f'    Language("{code}", "{name}", {str(rtl).lower()}),')
    kotlin += [")", "", "/** Each language's translations, a JSON object in the raw resources. Polish has none. */",
               "val LANGUAGE_RESOURCES: Map<String, Int> = mapOf("]
    for code, _, _ in LANGUAGES[1:]:
        kotlin.append(f'    "{code}" to R.raw.i18n_{code},')
    kotlin.append(")")
    (ROOT / "android/app/src/main/java/org/komunikator/app/I18nData.kt").write_text(
        "\n".join(kotlin) + "\n", encoding="utf-8")


if __name__ == "__main__":
    found = collect()
    if sys.argv[1:] == ["missing"]:
        for code, _, _ in LANGUAGES[1:]:
            table = load(code)
            missing = [key for key in found if key not in table]
            print(f"== {code}: {len(missing)}")
            for key in missing:
                print(f"  {key}")
        sys.exit(0)
    if sys.argv[1:] == ["keys"]:
        print(json.dumps(list(found), ensure_ascii=False, indent=1))
        sys.exit(0)
    bad = check(found)
    write(found)
    print(f"{len(found)} texts, {len(LANGUAGES)} languages" + (f", {bad} incomplete" if bad else ""))
