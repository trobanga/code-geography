/**
 * The cover of the diagram for a step that replaces the whole picture (a level button, "+", "−").
 * The cut of such a step blocks the main thread, so the cover must be on the screen before it.
 */
export function cover(d: { paint: () => Promise<void>; show: (on: boolean) => void }) {
  return {
    /** Shows the cover, waits for its paint, then runs the step. */
    async step(run: () => boolean): Promise<void> {
      d.show(true);
      await d.paint();
      let changed = false;
      try {
        changed = run();
      } finally {
        if (!changed) d.show(false);
      }
    },
    /** The layout that follows a step is there, or it failed. */
    landed(): void {
      d.show(false);
    },
  };
}
