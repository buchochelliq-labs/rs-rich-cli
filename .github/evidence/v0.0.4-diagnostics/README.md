# Input diagnostics evidence

Actual CLI output from the development binary, using a BOM-marked UTF-16 CSV.
`utf16-csv.svg` is the CLI's own export; `stdout.txt` preserves redirected output.

```bash
python3 - <<'PYTHON'
from pathlib import Path
Path('/tmp/v004-demo.csv').write_bytes('name,city,status\nZoë,Montréal,ready\n田中,東京,verified\nMarta,Kraków,ready\n'.encode('utf-16'))
PYTHON
rich --csv /tmp/v004-demo.csv --encoding utf-16 --width 68 \
  --title 'UTF-16 input · decoded explicitly' \
  --caption 'Unicode preserved · strict decoding' --export-svg utf16-csv.svg
```

Regression tests cover both byte orders, BOM/headerless input, default replacement,
strict UTF-8, stdin and a real local HTTP response, unused-option errors,
image hints and misleading suffixes. Decoder tests reject odd bytes, unpaired
surrogates and conflicting BOMs, and preserve explicit UTF-16LE BOM + NUL.
A duplicate-BOM regression ensures explicit stdin consumes only one BOM.

Independent review findings were fixed: explicit-endian BOM ambiguity, duplicate
BOM stripping on stdin, and an unnecessary whole-input copy on default reads.
