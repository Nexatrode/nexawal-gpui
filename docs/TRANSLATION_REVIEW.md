# Translation review status

The October 12, 2026 audit fills missing desktop UI keys and untranslated prose
using existing app translations first, then machine-generated drafts. This
includes maintenance/cache, sending, recovery and rate/privacy disclosures.
These are drafts, not fluent-speaker sign-off. Review security-critical wording
before claiming native-language quality. Full bundled legal documents and raw
native diagnostics remain English; URLs, addresses and ISO currency codes are
not translated.

`python3 scripts/check-l10n.py` checks coverage, duplicate JSON keys, nonblank
values, active literal lookups, printf arguments and numbered interpolation
arguments. Optional `--ios` and `--android` checkout paths check all three apps.
`python3 scripts/test-l10n.py` tests argument checks and the mobile catalog import.
`cargo test --locked --features ui-tests` includes locale/coverage tests.

`scripts/update-l10n.sh` preserves desktop-only entries rather than overwriting
them with the mobile catalog. Set `NEXAWAL_XCSTRINGS` to the intended iOS catalog;
do not accidentally import an unrelated feature checkout.

macOS uses preferred system languages when no locale environment override is
set. Linux uses LANGUAGE/LC_ALL/LC_MESSAGES/LANG, including preference lists and
Chinese/legacy language aliases. Environment-only locale configuration remains
the fallback on other desktop platforms.
