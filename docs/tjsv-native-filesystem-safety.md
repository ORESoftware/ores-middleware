# TJSV native evidence filesystem safety

This supplements [native contract admission](tjsv-native-admission.md). Linear: DEN-3828.

Both fixed-path tasks in `scripts/tjsv-native-conformance.mjs` reject linked output
ancestors and report files, including hard links and dangling symlinks. Existing
output leaves must be singly linked regular files. FIFOs, directories and other
special files are not report destinations. Parent components are checked before
creating descendants; no recursive creation follows an unchecked parent link.

JSON is written into an exclusively created, mode-0600 temporary file in the
validated destination directory, flushed and closed, then renamed into place.
Temporary files are cleaned up on error. Existing reports are not truncated in
place. Input files must be bounded regular files before opening, are opened with
no-follow/nonblocking flags, and their device/inode identity is checked after
opening. Nonblocking open prevents a substituted FIFO leaf from waiting for a
writer. The existing 4 MiB bounded read and size-change checks remain in force.

## Trust boundary

The checkout and its directory tree require one trusted writer. These checks
reject pre-existing filesystem hazards; portable Node path operations are not a
sandbox against a hostile concurrent process renaming ancestor directories.
They do not authenticate an attacker-controlled workflow or provide power-loss
recovery guarantees. Run the gate in an isolated, freshly checked-out workspace.

When an output path is unsafe, the task fails without following it to write a
failure tombstone. A receipt's mere existence is never success: require the
successful current workflow, source/IR binding and complete admission report.
Ordinary reused-output and missing-tool failures still invalidate regular old
passing reports, as covered by the existing I/O tests.

## Regression coverage

Run `node --test tests/tjsv-native-*.test.mjs`.

The new subprocess suite exercises both tasks against linked output ancestors,
symlink/hardlink/dangling-symlink reports and FIFO outputs; it also covers FIFO
input and normal complete JSON writes without temporary-file residue. Against
the original script blob `04fa8bd99e29f3c8977890d638d5cbb0e5758707`, 13 safety
assertions failed and the normal-path test passed. The fixed version passes all
14 new tests and all 48 existing admission/I/O tests on Node 22.16.0/Linux.
These synthetic filesystem tests do not replace hosted compiler/native evidence.

No TypeSpec/JSON Schema authority, TJSV revision, required adapter inventory or
native execution command is relaxed by this change. See the native-admission
scope limits before making claims about additional frameworks or transports.

API reference: https://nodejs.org/docs/latest-v22.x/api/fs.html
