# ZBrush test corpus

This directory holds **no ZBrush files**: only the registry template
`corpus.config.json`. Rules (see `docs/formats/zbrush/LEGAL.md` §5):

- Real files are listed by absolute path in a **private copy** of the
  config, selected with `OXY_ZTL_CORPUS=/path/to/my.corpus.json`.
- Only `synthetic` files (trivially simple, made from scratch, CC0) may be
  committed, each with a `.gitignore` negation and a config entry.
- `scripts/check_corpus_ip.py` enforces this in CI.

Run the corpus tests (Layer-1 echo and stream checks on every existing file):

```bash
OXY_ZTL_CORPUS=/path/to/my.corpus.json cargo test -p oxy-ztl --test corpus -- --nocapture
```
