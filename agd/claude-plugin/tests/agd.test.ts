import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'

// What the agd binary prints, keyed by its arguments. A test answers the
// plugin's `$.process.run` with these instead of running agd.
type Outputs = Record<string, { stdout?: string; stderr?: string; exitCode?: number }>

function agd(on: On, outputs: Outputs, calls: string[] = []) {
  on('process.run', async (_, e) => {
    const args = e.argv.slice(1).join(' ')
    calls.push(args)
    const output = outputs[args]
    if (e.argv[0] !== 'agd' || !output) {
      return { value: result(2, '', 'Error: no output for ' + args) }
    }
    return { value: result(output.exitCode ?? 0, output.stdout ?? '', output.stderr ?? '') }
  })
}

function result(exitCode: number, stdout: string, stderr: string) {
  return { exitCode, stdout, stderr, isStdoutTruncated: false, isStderrTruncated: false }
}

// A slash command as the person types it at the prompt.
function typed(command: string, args = '') {
  return { command, args, origin: { kind: 'composer' as const }, presentation: { isFullscreen: true, columns: 120 } }
}

// Claude Code's side of opening and closing a pane, which a test supplies.
function panes(on: On) {
  on('ui.open', async () => ({ value: { isPlaced: true } }))
  on('ui.close', async () => ({ value: undefined }))
}

function item(todo: string | null, title: string, extra: Record<string, unknown> = {}) {
  return {
    schema: 'agd.item/v1',
    ref: '/org/tasks.org:1#sha256:0',
    id: null,
    file: '/org/tasks.org',
    line: 1,
    olpath: [title],
    todo,
    done: false,
    priority: null,
    title,
    tags: [],
    inherited_tags: [],
    category: 'tasks',
    planning: {},
    ...extra,
  }
}

const DASHBOARD = JSON.stringify([
  {
    header: 'Next Tasks',
    from: null,
    to: null,
    entries: [],
    items: [item('NEXT', 'Write the report', { priority: 'A', tags: ['work'] })],
    color: 'blue',
  },
  { header: 'Waiting', from: null, to: null, entries: [], items: [], color: 'yellow' },
  {
    header: 'Cancelled',
    from: null,
    to: null,
    entries: [],
    items: [item('CANCELLED', 'Old idea <2026-10-01 Thu>', { done: true })],
    color: 'dim-red',
  },
])

const VIEWS = JSON.stringify([
  { key: 'd', desc: 'Dashboard', start: true },
  { key: 'e', desc: 'Low effort', start: false },
])

const LOW_EFFORT = JSON.stringify([
  {
    header: 'Quick',
    from: null,
    to: null,
    entries: [],
    items: [item('NEXT', 'Reply to the review')],
    color: 'blue',
  },
])

const PANE = {
  title: 'agd',
  isFocused: true,
  bodyColumns: 70,
  placement: 'dock' as const,
  scroll: { offset: 0, bodyRows: 30 },
  view: {},
}

for (const surface of ['terminal', 'desktop'] as const) {
  test(`/agd opens the start view with uppercase, counted sections (${surface})`, async ($, on) => {
    panes(on)
    agd(on, {
      'agenda --status': { stdout: '1 today · 3 inbox\n' },
      'agenda --views --json': { stdout: VIEWS },
      'agenda --view d --json': { stdout: DASHBOARD },
    })
    await $.command.run(typed('agd'))
    const ui = await $.ui.mount({ plugin: 'agd', surface, component: 'Pane', requestId: 'agd', props: PANE })
    expect(await ui.find({ type: 'Text', text: 'NEXT TASKS  1' })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /NEXT \[#A\] Write the report :work:/ })).toBeDefined()
    // An empty section has no count and is dimmed.
    const waiting = await ui.find({ type: 'Text', text: 'WAITING' })
    expect(waiting?.props.dimColor).toBe(true)
    // Done rows are dimmed and lose their active timestamp, as in agd's lists.
    const done = await ui.find({ type: 'Text', text: /Old idea$/ })
    expect(done?.props.dimColor).toBe(true)
    // The resolved section color from agd reaches the header.
    expect((await ui.find({ type: 'Text', text: 'CANCELLED  1' }))?.props.color).toBe('#b07076')
    await ui.unmount()
  })
}

test('without a start_view the pane shows today\'s agenda by day', async ($, on) => {
  const entry = {
    schema: 'agd.agenda-entry/v1',
    item: item('TODO', 'Call the bank'),
    occurrence: { kind: 'scheduled', date: '2026-10-15', time: '10:00', extra: 'Scheduled:' },
  }
  panes(on)
  agd(on, {
    'agenda --status': { stdout: '1 today · 0 inbox\n' },
    'agenda --views --json': { stdout: JSON.stringify([{ key: 'n', desc: 'Next', start: false }]) },
    'agenda --span day --json': { stdout: JSON.stringify([entry]) },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect(await ui.find({ type: 'Text', text: 'TODAY  1' })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: 'Thu 15 Oct' })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: /10:00\s+TODO Call the bank/ })).toBeDefined()
  await ui.unmount()
})

for (const surface of ['terminal', 'desktop'] as const) {
  test(`the tabs switch between saved views and today (${surface})`, async ($, on) => {
    panes(on)
    agd(on, {
      'agenda --status': { stdout: '1 today · 3 inbox\n' },
      'agenda --views --json': { stdout: VIEWS },
      'agenda --view d --json': { stdout: DASHBOARD },
      'agenda --view e --json': { stdout: LOW_EFFORT },
      'agenda --span day --json': { stdout: '[]' },
    })
    await $.command.run(typed('agd'))
    const ui = await $.ui.mount({ plugin: 'agd', surface, component: 'Pane', requestId: 'agd', props: PANE })
    // The start view's tab is the open one; the others are dimmed.
    expect((await ui.find({ key: 'view-d' }))?.props.dimColor).toBe(false)
    expect((await ui.find({ key: 'view-e' }))?.props.dimColor).toBe(true)
    expect((await ui.find({ key: 'view-e' }))?.props.hotkey).toBe('e')

    await ui.press({ key: 'view-e' })
    expect(await ui.find({ type: 'Text', text: 'QUICK  1' })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /NEXT TASKS/ })).toBeUndefined()

    await ui.press({ key: 'view-today' })
    expect(await ui.find({ type: 'Text', text: 'TODAY' })).toBeDefined()
    await ui.unmount()
  })
}

test('a view keyed a or g takes that key from Today or refresh', async ($, on) => {
  panes(on)
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --views --json': {
      stdout: JSON.stringify([
        { key: 'a', desc: 'All', start: true },
        { key: 'g', desc: 'Goals', start: false },
      ]),
    },
    'agenda --view a --json': { stdout: '[]' },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect((await ui.find({ key: 'view-a' }))?.props.hotkey).toBe('a')
  expect((await ui.find({ key: 'view-today' }))?.props.hotkey).toBeUndefined()
  expect((await ui.find({ key: 'view-g' }))?.props.hotkey).toBe('g')
  expect((await ui.find({ key: 'refresh' }))?.props.hotkey).toBeUndefined()
  await ui.unmount()
})

test('the band above the prompt shows the day\'s counts', async ($, on) => {
  panes(on)
  // No other mod draws in the band.
  on('ui.render', { component: 'AbovePrompt' }, async ($, e) => $.ui.resolve(e).Box({ children: [] }))
  agd(on, {
    'agenda --status': { stdout: '2 today · 5 inbox\n' },
    'agenda --views --json': { stdout: '[]' },
    'agenda --span day --json': { stdout: '[]' },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({
    plugin: 'agd',
    surface: 'terminal',
    component: 'AbovePrompt',
    props: { hasSurvey: false, isWorking: false, maxRows: 3, bodyColumns: 80, scroll: { offset: 0, bodyRows: 3 }, view: {} },
  })
  expect(await ui.find({ type: 'Text', text: 'agd · 2 today · 5 inbox' })).toBeDefined()
  await ui.unmount()
})

test('/capture runs agd capture with the key and the rest of the line', async ($, on) => {
  const calls: string[] = []
  agd(
    on,
    {
      'agenda --status': { stdout: '1 today · 1 inbox\n' },
      'capture t buy milk due:fri': { stdout: '/home/me/org/inbox.org:7#sha256:abc\n' },
    },
    calls,
  )
  const captured = await $.command.run(typed('capture', 't buy milk due:fri'))
  expect(captured.text).toBe('captured to inbox.org:7')
  expect(calls).toContain('capture t buy milk due:fri')
})

test('/capture reports agd\'s own error and its usage', async ($, on) => {
  agd(on, {
    'agenda --status': { stdout: '' },
    'capture zz nothing': { exitCode: 1, stderr: "Error: capture template 'zz' is not configured" },
  })
  expect((await $.command.run(typed('capture', 'zz nothing'))).text).toBe(
    "capture template 'zz' is not configured",
  )
  expect((await $.command.run(typed('capture', ''))).text).toMatch(/^usage: \/capture KEY text/)
})

test('a missing agd is said in the pane', async ($, on) => {
  panes(on)
  on('process.run', async () => ({ deny: 'spawn agd ENOENT' }))
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect(await ui.find({ type: 'Text', text: /^cannot run agd/ })).toBeDefined()
  await ui.unmount()
})
