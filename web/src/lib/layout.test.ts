import { describe, expect, it } from 'vitest';
import type { ElkNode } from 'elkjs/lib/elk-api';
import { toLayout } from './layout';

describe('toLayout', () => {
  it('leaves out the boxes that exist only in the layout and keeps parent and depth of the others', () => {
    const leaf = (id: string, x: number): ElkNode => ({ id, x, y: 0, width: 10, height: 10 });
    const res: ElkNode = {
      id: 'root',
      width: 100,
      height: 50,
      children: [
        {
          id: 'layout:act:',
          children: [{ ...leaf('m', 0), children: [{ id: 'layout:idle:m', children: [leaf('a', 5)] }] }],
        },
      ],
    };
    expect(toLayout(res).nodes.map((n) => [n.id, n.parent, n.depth, n.container])).toEqual([
      ['m', null, 0, true],
      ['a', 'm', 1, false],
    ]);
  });
});
