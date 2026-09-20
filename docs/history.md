# History supplements

History supplements add concepts connected to the initial result by active historical association members. They reuse the typed member index; no separate history file is needed.

| Form | Association selection |
|---|---|
| `{{ + HISTORY-MIN }}` | SAME AS |
| `{{ + HISTORY-MOD }}` | SAME AS, REPLACED BY, WAS A, PARTIALLY EQUIVALENT TO |
| `{{ + HISTORY-MAX }}` | Descendants of the historical association reference set concept |
| `{{ + HISTORY }}` or `{{ + HISTORY (*) }}` | Same as MAX |
| `{{ + HISTORY (expression) }}` | Refsets selected by the expression |

Suffixes also accept an underscore. A supplement follows the focus expression's concept and description filters. Parentheses allow the supplemented result to participate in a larger expression:

```text
<< 195967001 |Asthma| {{ + HISTORY-MOD }}
(<< 195967001 {{ + HISTORY-MIN }}) MINUS 67415000
195967001 {{ + HISTORY (900000000000526001) }}
```

Each supplement takes one association step. It tests targets against the original result, not concepts added during the scan. Nesting two supplements deliberately applies two steps.

The [specification's MOVED FROM guidance](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.11-history-supplements) recommends treating these reversed associations as SAME AS. History evaluation includes them when SAME AS or MOVED FROM is selected, reversing their direction. It ignores MOVED TO associations. Generic member queries still expose the original RF2 rows, so this normalisation applies to history evaluation only.

Synthetic tests cover profiles, subsets, inactive members, reversed associations, one-step behaviour, nesting and evaluation limits. `scripts/check_history_rf2.py` independently scans the licensed RF2 archive to check complete result sets. Raw terminology results remain in ignored local files.

The [UK release checks](../validation/history-results.json) match all 52 complete RF2 sets and all four probes accepted by OneLondon's Ontoserver. The [1,000-expression benchmark](../validation/history-corpus-results.json) evaluates every case and preserves the previous 960 result digests. Five shuffled batches on one CPU and a 1 GiB limit produced a 1.73 ms median request, a 2.87 second median batch and a 533.18 MiB charged peak. These measurements use the existing `v1-members` store, which predates the identifier section; they are not final full-language resource claims.
