# 19. The kind fold: a mixed container shows its packages and folds the rest

Status: accepted (2026-11-10)

## Context
Since packages nest (ADR 18), an open package shows its sub-packages and its own types
side by side. On a test repository this gave 87 boxes in the package overview: the
layer of the sub-packages was buried under the types of their parents. ADR 11 folds by
interest only, so it does nothing here, because there is no interest.

## Decision
`Focus` has a flag `fold_mixed` (default off). With it, an expanded container folds by
kind when all of these hold:

- it has at least one child of kind `package` and at least one child of another kind;
- no child of it is kept by the interest, that is, no child is a node of interest or an
  ancestor of one;
- it is not in `unfolded`.

It then shows its package children and the placeholder `rest:<container id>`. The other
children are folded as in ADR 11: `rest[<id>]` counts them, and the edges of them and of
their descendants end at the placeholder.

Rules around it:

- A container without a package child (a leaf package, a type) shows all children. A
  container with only package children has no placeholder.
- A container with a child that the interest keeps follows ADR 11 unchanged.
- A child that is kept only because it lies below an explicit node is not protected: the
  explicit node does not stop the kind fold of the packages below it. An explicit node
  or a node of interest inside the container does.
- The flag works with an empty focus, and an empty focus without the flag is still the
  plain cut.
- `View` does not change. `expandable` and `changes` treat the placeholder as in ADR 11.
- The WASM binding passes the flag as the last parameter of `focusView`.

## Consequences
- The viewer can show a package layer by layer: sub-packages first, the own types of a
  package behind one placeholder that the user can unfold. It decides when to set the
  flag, for example by depth step.
- As in ADR 11, the lines of the placeholder summarize unrelated nodes and should be
  drawn as background.
- A package with many sub-packages and one stray type shows the type only as a count.

## The viewer
- It always cuts with the flag, also without a focus. The focus bar stays about nodes and
  changes. `unfolded` is depth state beside `expanded`, in the hash as `unfold=` (old focus
  hashes read the same key).
- A placeholder shows the kinds of its folded children: `12 types`, `3 types, 2 functions`.
- "+" ranks a kind placeholder by the coarsest kind it folds and unfolds it together with the
  closed containers of that rank: the leaf packages and the "N types" open in one step. A
  placeholder whose container shows a non-package or folds a package is from the focus and
  is skipped.
- "−" after "+" restores both sets. Without that history it closes the open leaves and folds
  again every unfolded container whose own children are as fine as the closed layer or finer.
- Levels: `packages` opens the modules and every package with a package child; `types` and
  `functions` also unfold every mixed container.
- Lines of a kind placeholder are not background: without a focus `rest:<c>` lifts like a
  child of `c`. With a focus they are background as in ADR 11.
