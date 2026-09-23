# Working on this repository

Follow [CONTRIBUTING.md](CONTRIBUTING.md): its build commands, engineering rules
and commit conventions apply to automated contributors too. [SKILL.md](SKILL.md)
is the workflow for using the engine rather than changing it.

- Never commit SNOMED CT content: RF2 files, extracts or built indexes.
- Never log or save a TRUD API response. Its download URLs contain the API key.
- ECL 2.3 is the target. Report unsupported ECL explicitly rather than returning
  a partial answer.
