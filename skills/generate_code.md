# Generate code

Write the change, then stop.

- Read the surrounding code first (`list_dir`, `read_file`); match its style, naming and error
  handling instead of inventing a new pattern.
- Prefer the smallest diff that solves the problem; no speculative abstraction, no "for later" code.
- Write with `write_file`; shell is only for building, testing and formatting.
- Run the project's own checks when they exist (build, tests, lint) before declaring it done.
- Close with what changed and how you verified it — one or two lines.
