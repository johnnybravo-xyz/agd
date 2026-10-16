import { expect, mock, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
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

// Claude Code's side of opening and closing a pane, which a test supplies,
// and the person's Claude Code theme as /config lists it.
function panes(on: On, claudeTheme = 'dark') {
  on('ui.open', async () => ({ value: { isPlaced: true } }))
  on('ui.close', async () => ({ value: undefined }))
  on('config.list', async () => ({
    value: [
      {
        key: 'theme',
        label: 'Theme',
        kind: 'choice' as const,
        value: claudeTheme,
        provider: { plugin: 'engine', tier: 'core' as const },
        isLocked: false,
      },
    ],
  }))
}

// A query for a Text showing exactly `text`; a string query matches any
// Text containing it, outermost first, which a row's outer Text would.
function exactly(text: string) {
  return { type: 'Text', text: new RegExp('^' + text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '$') }
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
    items: [item('NEXT', 'Write the [[https://example.com][report]]', { priority: 'A', tags: ['work'] })],
    groups: [],
    color: 'blue',
  },
  { header: 'Waiting', from: null, to: null, entries: [], items: [], groups: [], color: 'yellow' },
  {
    header: 'Cancelled',
    from: null,
    to: null,
    entries: [],
    items: [item('CANCELLED', 'Old idea <2026-10-01 Thu>', { done: true })],
    groups: [],
    color: 'dim-red',
  },
])

// `agd agenda --views --json`: the saved views and the TUI's display settings.
function viewsJson(views: { key: string; desc: string; start: boolean }[], display: Record<string, unknown> = {}) {
  return JSON.stringify({
    views,
    theme: 'auto',
    nerd_font: false,
    keyword_colors: { CANCELLED: 'dim-red', DONE: 'green', NEXT: 'blue', TODO: 'orange' },
    ...display,
  })
}

const VIEWS = viewsJson(
  [
    { key: 'd', desc: 'Dashboard', start: true },
    { key: 'e', desc: 'Low effort', start: false },
  ],
  { nerd_font: true },
)

const LOW_EFFORT = JSON.stringify([
  {
    header: 'Quick',
    from: null,
    to: null,
    entries: [],
    items: [item('NEXT', 'Reply to the review')],
    groups: [],
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
  test(`/agd opens the start view drawn as the TUI draws it (${surface})`, async ($, on) => {
    panes(on)
    agd(on, {
      'agenda --status': { stdout: '1 today · 3 inbox\n' },
      'agenda --views --json': { stdout: VIEWS },
      'agenda --view d --json': { stdout: DASHBOARD },
    })
    await $.command.run(typed('agd'))
    const ui = await $.ui.mount({ plugin: 'agd', surface, component: 'Pane', requestId: 'agd', props: PANE })
    // Uppercase headers in the section's color with a muted count, in the
    // TUI's dark palette.
    const next = await ui.find(exactly('NEXT TASKS'))
    expect(next?.props.color).toBe('#6ea8fe')
    expect(next?.props.bold).toBe(true)
    expect((await ui.find(exactly('  1')))?.props.color).toBe('#808896')
    // An empty section has no count and is muted.
    expect((await ui.find(exactly('WAITING')))?.props.color).toBe('#808896')
    // The keyword in its configured color, a link read as its description,
    // the priority as a rounded chip (nerd_font) and the tags in accent.
    expect((await ui.find(exactly('NEXT ')))?.props.color).toBe('#6ea8fe')
    expect(await ui.find(exactly('Write the report'))).toBeDefined()
    const chip = await ui.find(exactly('A'))
    expect(chip?.props.color).toBe('#e5b567')
    expect(chip?.props.backgroundColor).toBe('#20252e')
    expect(await ui.find(exactly('\ue0b6'))).toBeDefined()
    expect((await ui.find(exactly('  #work')))?.props.color).toBe('#6ea8fe')
    // A done row: its keyword in dim-red, not bold; its title muted, struck
    // through and without its active timestamp.
    const cancelled = await ui.find(exactly('CANCELLED '))
    expect(cancelled?.props.color).toBe('#b07076')
    expect(cancelled?.props.bold).toBe(false)
    const done = await ui.find(exactly('Old idea'))
    expect(done?.props.strikethrough).toBe(true)
    expect(done?.props.color).toBe('#808896')
    expect((await ui.find(exactly('CANCELLED')))?.props.color).toBe('#b07076')
    await ui.unmount()
  })
}

test('agd\'s theme setting picks the palette over Claude Code\'s', async ($, on) => {
  panes(on, 'dark')
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --views --json': { stdout: viewsJson([{ key: 'd', desc: 'Dashboard', start: true }], { theme: 'light' }) },
    'agenda --view d --json': { stdout: DASHBOARD },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect((await ui.find(exactly('NEXT TASKS')))?.props.color).toBe('#2563c4')
  // Without nerd_font the chip is square: the label padded on the surface.
  expect((await ui.find(exactly(' A ')))?.props.backgroundColor).toBe('#eef0f3')
  await ui.unmount()
})

for (const [claudeTheme, accent] of [
  ['light-daltonized', '#2563c4'],
  ['dark-ansi', 'blueBright'],
] as const) {
  test(`with theme auto the pane follows Claude Code's ${claudeTheme} theme`, async ($, on) => {
    panes(on, claudeTheme)
    agd(on, {
      'agenda --status': { stdout: '' },
      'agenda --views --json': { stdout: viewsJson([{ key: 'd', desc: 'Dashboard', start: true }]) },
      'agenda --view d --json': { stdout: DASHBOARD },
    })
    await $.command.run(typed('agd'))
    const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
    expect((await ui.find(exactly('NEXT TASKS')))?.props.color).toBe(accent)
    await ui.unmount()
  })
}

test('without a start_view the pane shows today\'s agenda with org-agenda leads', async ($, on) => {
  const entries = [
    {
      schema: 'agd.agenda-entry/v1',
      item: item('TODO', 'Call the bank'),
      occurrence: { type: 'scheduled', date: '2026-10-15', time: '10:00', extra: 'Scheduled:' },
    },
    {
      schema: 'agd.agenda-entry/v1',
      item: item('TODO', 'File the taxes', { category: 'home' }),
      occurrence: { type: 'past-scheduled', date: '2026-10-15', time: null, extra: 'Sched. 2x:' },
    },
    {
      schema: 'agd.agenda-entry/v1',
      item: item('TODO', 'Pay the rent'),
      occurrence: { type: 'deadline', date: '2026-10-15', time: null, extra: '1 d. ago:' },
    },
    {
      schema: 'agd.agenda-entry/v1',
      item: item('TODO', 'Renew the lease'),
      occurrence: { type: 'deadline', date: '2026-10-15', time: null, extra: 'Deadline:' },
    },
  ]
  panes(on)
  agd(on, {
    'agenda --status': { stdout: '1 today · 0 inbox\n' },
    'agenda --views --json': { stdout: viewsJson([{ key: 'n', desc: 'Next', start: false }]) },
    'agenda --span day --json': { stdout: JSON.stringify(entries) },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect(await ui.find(exactly('TODAY'))).toBeDefined()
  expect(await ui.find(exactly('Thu 15 Oct'))).toBeDefined()
  expect((await ui.find(exactly('tasks:      ')))?.props.color).toBe('#808896')
  expect(await ui.find(exactly('10:00...... '))).toBeDefined()
  // An overdue lead is red, a deadline's amber, the rest muted; the first
  // keyword of a sequence is orange, not bold.
  expect((await ui.find(exactly('Sched. 2x:  ')))?.props.color).toBe('#f07178')
  expect((await ui.find(exactly('1 d. ago:   ')))?.props.color).toBe('#f07178')
  expect((await ui.find(exactly('Deadline:   ')))?.props.color).toBe('#e5b567')
  expect((await ui.find(exactly('Scheduled:  ')))?.props.color).toBe('#808896')
  const todo = await ui.find(exactly('TODO '))
  expect(todo?.props.color).toBe('#e5b567')
  expect(todo?.props.bold).toBe(false)
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
    expect(await ui.find(exactly('QUICK'))).toBeDefined()
    expect(await ui.find(exactly('NEXT TASKS'))).toBeUndefined()

    await ui.press({ key: 'view-today' })
    expect(await ui.find(exactly('TODAY'))).toBeDefined()
    await ui.unmount()
  })
}

test('a view keyed a or g takes that key from Today or refresh', async ($, on) => {
  panes(on)
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --views --json': {
      stdout: viewsJson([
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
    'agenda --views --json': { stdout: viewsJson([]) },
    'agenda --span day --json': { stdout: '[]' },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({
    plugin: 'agd',
    surface: 'terminal',
    component: 'AbovePrompt',
    props: { hasSurvey: false, isWorking: false, maxRows: 3, bodyColumns: 80, scroll: { offset: 0, bodyRows: 3 }, view: {} },
  })
  expect(await ui.find(exactly('agd · 2 today · 5 inbox'))).toBeDefined()
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

const BAND = { hasSurvey: false, isWorking: false, maxRows: 3, bodyColumns: 80, scroll: { offset: 0, bodyRows: 3 }, view: {} }

// The band's agd line as drawn now, or undefined when the band shows none.
async function bandLine($: Engine) {
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'AbovePrompt', props: BAND })
  const line = await ui.find({ type: 'Text', text: /^agd · / })
  await ui.unmount()
  return line?.text
}

// Claude Code's side of a session start: it takes the commands and answers.
function sessionStart(on: On, registered: string[]) {
  on('session.start', async (_, e) => ({ cwd: e.cwd }))
  on('command.register', async (_, e) => {
    registered.push(e.name)
    return { value: { command: e.name } }
  })
  on('ui.render', { component: 'AbovePrompt' }, async ($, e) => $.ui.resolve(e).Box({ children: [] }))
}

// A process.run answer held until the test lets it go.
function held() {
  let release = () => {}
  const gate = new Promise<void>((resolve) => {
    release = resolve
  })
  return { gate, release: () => release() }
}

test('the session starts without waiting for agd and reads it after', async ($, on) => {
  const clock = mock.clock(on)
  const registered: string[] = []
  sessionStart(on, registered)
  const status = held()
  const calls: string[] = []
  on('process.run', async (_, e) => {
    calls.push(e.argv.slice(1).join(' '))
    await status.gate
    return { value: result(0, '2 today · 5 inbox\n', '') }
  })
  // agd has not answered, yet the session has started with the commands.
  await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  expect(registered).toEqual(['agd', 'capture'])
  expect(calls).toEqual([])

  await clock.settle()
  expect(calls).toEqual(['agenda --status'])
  status.release()
  await clock.settle()
  expect(await bandLine($)).toBe('agd · 2 today · 5 inbox')
})

test('a write tool answers at once and the band catches up; reads refresh nothing', async ($, on) => {
  const clock = mock.clock(on)
  const calls: string[] = []
  agd(on, { 'agenda --status': { stdout: '1 today · 1 inbox\n' } }, calls)
  on('ui.render', { component: 'AbovePrompt' }, async ($, e) => $.ui.resolve(e).Box({ children: [] }))
  on('tool.call', async () => ({ result: { content: [{ type: 'text', text: '{"schema":"agd.mutation/v1"}' }] } }))

  const answered = await $.tool.call({ tool: 'mcp__plugin_agd_agd__capture', template: 't', title: 'Buy milk' })
  expect(answered.result).toEqual({ content: [{ type: 'text', text: '{"schema":"agd.mutation/v1"}' }] })
  expect(calls).toEqual([])
  await clock.settle()
  expect(calls).toEqual(['agenda --status'])
  expect(await bandLine($)).toBe('agd · 1 today · 1 inbox')

  for (const read of ['agenda', 'query', 'describe']) {
    await $.tool.call({ tool: 'mcp__plugin_agd_agd__' + read })
  }
  await clock.settle()
  expect(calls).toEqual(['agenda --status'])
})

test('a slower, older refresh never draws over a newer one', async ($, on) => {
  const clock = mock.clock(on)
  on('ui.render', { component: 'AbovePrompt' }, async ($, e) => $.ui.resolve(e).Box({ children: [] }))
  on('tool.call', async () => ({ result: { content: [] } }))
  const first = held()
  let runs = 0
  on('process.run', async () => {
    runs += 1
    if (runs === 1) {
      await first.gate
      return { value: result(0, '1 today\n', '') }
    }
    return { value: result(0, '2 today\n', '') }
  })
  await $.tool.call({ tool: 'mcp__plugin_agd_agd__set_state', ref: 'a', state: 'DONE' })
  await clock.settle()
  await $.tool.call({ tool: 'mcp__plugin_agd_agd__plan', ref: 'a', deadline: 'fri' })
  await clock.settle()
  expect(await bandLine($)).toBe('agd · 2 today')

  // The first refresh's answer arrives last, older than what is drawn.
  first.release()
  await clock.settle()
  expect(runs).toBe(2)
  expect(await bandLine($)).toBe('agd · 2 today')
})
