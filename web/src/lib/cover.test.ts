import { describe, expect, it } from 'vitest';
import { cover } from './cover';

/** A cover with a log of what happens, in order. */
function setup() {
  const log: string[] = [];
  const c = cover({
    paint: async () => {
      log.push('paint');
    },
    show: (on) => log.push(on ? 'show' : 'hide'),
  });
  return { c, log };
}

describe('cover', () => {
  it('is shown and painted before the step runs', async () => {
    const { c, log } = setup();
    await c.step(() => {
      log.push('run');
      return true;
    });
    expect(log).toEqual(['show', 'paint', 'run']);
  });

  it('goes away when the step changes nothing', async () => {
    const { c, log } = setup();
    await c.step(() => false);
    expect(log).toEqual(['show', 'paint', 'hide']);
  });

  it('stays until the layout of the step lands', async () => {
    const { c, log } = setup();
    await c.step(() => true);
    expect(log).toEqual(['show', 'paint']);
    c.landed();
    expect(log).toEqual(['show', 'paint', 'hide']);
  });

  it('goes away when the step throws', async () => {
    const { c, log } = setup();
    await expect(
      c.step(() => {
        throw new Error('cut failed');
      }),
    ).rejects.toThrow('cut failed');
    expect(log).toEqual(['show', 'paint', 'hide']);
  });
});
