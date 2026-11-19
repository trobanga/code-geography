import { describe, expect, it } from 'vitest';
import { boxIndex, findBoxes } from './boxSearch';
import { buildTree } from './expansion';
import type { GraphNode } from './types';

const nodes: GraphNode[] = [
  { id: 'm', parent: null, kind: 'module', name: 'm' },
  { id: 'p', parent: 'm', kind: 'package', name: 'p' },
  { id: 'UserService', parent: 'p', kind: 'type', name: 'UserService' },
  { id: 'User', parent: 'p', kind: 'type', name: 'User' },
];
const index = boxIndex(buildTree(nodes));

describe('findBoxes', () => {
  it('gives no matches for a blank query', () => {
    expect(findBoxes(index, '', false)).toEqual({ matches: [], total: 0 });
    expect(findBoxes(index, '   ', false)).toEqual({ matches: [], total: 0 });
  });

  it('matches a word in the name case-insensitively', () => {
    const r = findBoxes(index, 'USER', false);
    expect(r.matches.map((m) => m.id).sort()).toEqual(['User', 'UserService']);
    expect(r.total).toBe(2);
  });

  it('needs every word in the name', () => {
    const idx = boxIndex(
      buildTree([
        { id: 'a', parent: null, kind: 'module', name: 'a' },
        { id: 'a.UserService', parent: 'a', kind: 'type', name: 'UserService' },
        { id: 'a.User', parent: 'a', kind: 'type', name: 'User' },
      ]),
    );
    expect(findBoxes(idx, 'user  service', false).matches.map((m) => m.id)).toEqual(['a.UserService']);
  });

  it('orders exact name, prefix, other; then shorter name, then path', () => {
    const idx = boxIndex(
      buildTree([
        { id: 'm', parent: null, kind: 'module', name: 'm' },
        { id: 'z', parent: 'm', kind: 'package', name: 'z' },
        { id: 'MyUserX', parent: 'm', kind: 'type', name: 'MyUserX' },
        { id: 'MyUser', parent: 'm', kind: 'type', name: 'MyUser' },
        { id: 'UserLongName', parent: 'm', kind: 'type', name: 'UserLongName' },
        { id: 'UserB', parent: 'm', kind: 'type', name: 'UserB' },
        { id: 'user@z', parent: 'z', kind: 'type', name: 'user' },
        { id: 'user@m', parent: 'm', kind: 'type', name: 'user' },
      ]),
    );
    expect(findBoxes(idx, 'User', false).matches.map((m) => m.id)).toEqual([
      'user@m',
      'user@z',
      'UserB',
      'UserLongName',
      'MyUser',
      'MyUserX',
    ]);
  });

  it('lists test code last and hidden while tests are off', () => {
    const idx = boxIndex(
      buildTree([
        { id: 'm', parent: null, kind: 'module', name: 'm' },
        { id: 'T', parent: 'm', kind: 'type', name: 'T', test: true },
        { id: 'T.fooTest', parent: 'T', kind: 'function', name: 'fooTest' },
        { id: 'fooLonger', parent: 'm', kind: 'type', name: 'fooLonger' },
        { id: 'foo', parent: 'm', kind: 'type', name: 'foo', test: true },
      ]),
    );
    const off = findBoxes(idx, 'foo', false).matches;
    expect(off.map((m) => [m.id, m.hidden])).toEqual([
      ['fooLonger', false],
      ['foo', true],
      ['T.fooTest', true],
    ]);
    const on = findBoxes(idx, 'foo', true).matches;
    expect(on.map((m) => [m.id, m.hidden])).toEqual([
      ['foo', false],
      ['T.fooTest', false],
      ['fooLonger', false],
    ]);
  });

  it('caps the matches at the limit but counts all', () => {
    const r = findBoxes(index, 'user', false, 1);
    expect(r.matches.map((m) => m.id)).toEqual(['User']);
    expect(r.total).toBe(2);
  });

  it('carries kind and the path of the ancestors', () => {
    const r = findBoxes(index, 'userservice', false);
    expect(r.matches).toEqual([{ id: 'UserService', name: 'UserService', kind: 'type', path: 'm › p', hidden: false }]);
    expect(findBoxes(index, 'm', false).matches.find((m) => m.id === 'm')).toMatchObject({ path: '', kind: 'module' });
  });

  it('shows the label of a package in the path, without the shared prefix', () => {
    const idx = boxIndex(
      buildTree([
        { id: 'm', parent: null, kind: 'module', name: 'm' },
        { id: 'pkg:1', parent: 'm', kind: 'package', name: 'org.acme.a' },
        { id: 'pkg:2', parent: 'm', kind: 'package', name: 'org.acme.c' },
        { id: 'pkg:3', parent: 'pkg:1', kind: 'package', name: 'b' },
        { id: 'type:T', parent: 'pkg:3', kind: 'type', name: 'T' },
      ]),
    );
    expect(findBoxes(idx, 't', false).matches[0].path).toBe('m › a › b');
  });

  it('ranks the name equal to a multi-word query first, then the prefix of the first word', () => {
    const idx = boxIndex(
      buildTree(
        ['a user service b', 'user service', 'user other service', 'my user service'].map((name) => ({
          id: name,
          parent: null,
          kind: 'module' as const,
          name,
        })),
      ),
    );
    expect(findBoxes(idx, 'User   Service', false).matches.map((m) => m.id)).toEqual([
      'user service',
      'user other service',
      'my user service',
      'a user service b',
    ]);
  });

  it('breaks a tie of name and path by id, by code point', () => {
    const idx = boxIndex(
      buildTree([
        { id: 'm', parent: null, kind: 'module', name: 'm' },
        { id: 'b#2', parent: 'm', kind: 'function', name: 'go' },
        { id: 'B#1', parent: 'm', kind: 'function', name: 'go' },
        { id: 'a#3', parent: 'm', kind: 'function', name: 'go' },
      ]),
    );
    expect(findBoxes(idx, 'go', false).matches.map((m) => m.id)).toEqual(['B#1', 'a#3', 'b#2']);
  });
});

