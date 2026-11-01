# Review code

Find what is wrong; change only what the user asked you to change.

- Read the code and the code it calls before judging it (`read_file`, `list_dir`).
- Rank findings by what breaks first: correctness, data loss and security, then tests, then
  readability. Each finding gets `path:line`, what goes wrong, and the smallest fix.
- Say plainly when a concern is a style preference rather than a defect.
- Show the load path that proves a bug (caller, input, state) instead of describing it in the
  abstract; skip findings you cannot ground in the code.
- End with one line: ship, fix first, or needs a rewrite.
