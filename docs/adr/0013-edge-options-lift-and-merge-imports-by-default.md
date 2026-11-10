# 13. Lift and merge imports are the default edge options

Status: accepted (2026-11-10)

## Context
The package level of the test repository has 72 lines and the type level has 368. Five
options were built as prototypes so that the user could try them: O1 lines on selection
only, O2 filter by kind and minimum count, O3 lift, O4 merge imports, O5 transitive
reduction.

## Decision
- O3 lift is on by default and stays a switch. A line is drawn between the outermost
  boxes that separate its ends. A selected node shows its real lines, routed with
  right angles like all other lines.
- O4 merge imports is on by default and stays a switch (an import that a use covers is
  not counted).
- O1 on selection stays a switch, off by default.
- The filter by edge kind stays. The minimum count is removed.
- O5 transitive reduction is removed.
- Under lift, a line of a placeholder of the focus cut (ADR 11) is dropped, not merged
  into the line of its ancestors. The viewer shows the number of dropped lines.

## Consequences
- Lift hides the exact end of a line that crosses a container. The user must select a
  node to see its targets.
- Transitive reduction hid true direct dependencies, and with cycles its result was not
  unique. A layering violation is exactly a line that it removed.
- A minimum count hides a single `extends`, which can be the most important line.
- The decision rests on line counts and on screenshots, not on a measured task. If one
  real question needs more than two clicks under lift, the default is wrong.
