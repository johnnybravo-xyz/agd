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
      return { value: result(2, '', 'error: no output for ' + args) }
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

// The pane reads the time as 12:00 on Thursday 15 October 2026, local.
const NOON = new Date(2026, 9, 15, 12, 0).getTime()

// Claude Code's side of opening and closing a pane, which a test supplies,
// the person's Claude Code theme as /config lists it, and its clock.
function panes(on: On, claudeTheme = 'dark') {
  mock.clock(on, { now: NOON })
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

// The palettes as `agd agenda --views --json` prints them from theme.rs.
const PALETTES = {
  dark: {
    accent: '#6ea8fe',
    overdue: '#f07178',
    due: '#e5b567',
    done: '#7ec68f',
    text: '#dee2e8',
    muted: '#808896',
    border: '#424a58',
    surface: '#20252e',
    selection: '#25344a',
    yellow: '#dcd664',
    dim_red: '#b07076',
    pink: '#e879b0',
    teal: '#5ec4a0',
    purple: '#b094f5',
    agents: ['#e879b0', '#b094f5', '#64bee6'],
  },
  light: {
    accent: '#2563c4',
    overdue: '#c53030',
    due: '#b06f00',
    done: '#248748',
    text: '#1f2329',
    muted: '#6c727e',
    border: '#c4c9d0',
    surface: '#eef0f3',
    selection: '#d6e2f2',
    yellow: '#787600',
    dim_red: '#b87878',
    pink: '#c83c82',
    teal: '#148c64',
    purple: '#6e50c8',
    agents: ['#c83c82', '#6e50c8', '#1478aa'],
  },
  ansi: {
    accent: 'blueBright',
    overdue: 'red',
    due: 'yellow',
    done: 'green',
    yellow: 'yellowBright',
    dim_red: 'red',
    pink: 'magenta',
    teal: 'cyan',
    purple: 'magentaBright',
    agents: ['magenta', 'magentaBright', 'cyanBright'],
  },
}

// `agd agenda --views --json`: the saved views and the TUI's display settings.
function viewsJson(views: { key: string; desc: string; start: boolean }[], display: Record<string, unknown> = {}) {
  return JSON.stringify({
    views,
    theme: 'auto',
    nerd_font: false,
    keyword_colors: { CANCELLED: 'dim-red', DONE: 'green', NEXT: 'blue', TODO: 'orange' },
    palettes: PALETTES,
    ...display,
  })
}

// `agd agenda --status --json`: the band's counts and the overdue rows, in
// the order agd gives them.
function statusJson(counts: Record<string, number> = {}, overdue: unknown[] = []) {
  return JSON.stringify({
    schema: 'agd.status/v1',
    overdue: overdue.length,
    today: 0,
    inbox: 0,
    unreviewed: 0,
    ...counts,
    overdue_entries: overdue,
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
    // the priority as a rounded chip (nerd_font) and the tags muted.
    expect((await ui.find(exactly('NEXT ')))?.props.color).toBe('#6ea8fe')
    expect(await ui.find(exactly('Write the report'))).toBeDefined()
    const chip = await ui.find(exactly('A'))
    expect(chip?.props.color).toBe('#e5b567')
    expect(chip?.props.backgroundColor).toBe('#20252e')
    expect(await ui.find(exactly('\ue0b6'))).toBeDefined()
    expect((await ui.find(exactly('  #work')))?.props.color).toBe('#808896')
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

test('tags are drawn in agd\'s notation', async ($, on) => {
  panes(on)
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --views --json': { stdout: viewsJson([{ key: 'd', desc: 'Dashboard', start: true }], { notation: 'org' }) },
    'agenda --view d --json': {
      stdout: JSON.stringify([
        {
          header: 'Next Tasks',
          from: null,
          to: null,
          entries: [],
          items: [item('NEXT', 'Write the report', { tags: ['work', 'ops'] })],
          groups: [],
          color: 'blue',
        },
      ]),
    },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect((await ui.find(exactly('  :work:ops:')))?.props.color).toBe('#808896')
  await ui.unmount()
})

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

test('an agd that prints no palettes is drawn in Claude Code\'s own theme colors', async ($, on) => {
  panes(on, 'dark')
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --views --json': { stdout: viewsJson([{ key: 'd', desc: 'Dashboard', start: true }], { palettes: undefined }) },
    'agenda --view d --json': { stdout: DASHBOARD },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect((await ui.find(exactly('NEXT TASKS')))?.props.color).toBe('suggestion')
  expect((await ui.find(exactly('WAITING')))?.props.color).toBe('inactive')
  await ui.unmount()
})

test('every role\'s color comes from the palette agd prints, by its theme.rs name', async ($, on) => {
  panes(on, 'dark')
  const palettes = { ...PALETTES, dark: { ...PALETTES.dark, dim_red: '#123456', accent: '#abcdef' } }
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --views --json': { stdout: viewsJson([{ key: 'd', desc: 'Dashboard', start: true }], { palettes }) },
    'agenda --view d --json': { stdout: DASHBOARD },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect((await ui.find(exactly('CANCELLED ')))?.props.color).toBe('#123456')
  expect((await ui.find(exactly('NEXT TASKS')))?.props.color).toBe('#abcdef')
  await ui.unmount()
})

test('16 colours dim muted text instead of picking a grey', async ($, on) => {
  panes(on, 'dark-ansi')
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --views --json': { stdout: viewsJson([{ key: 'd', desc: 'Dashboard', start: true }]) },
    'agenda --view d --json': { stdout: DASHBOARD },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  const waiting = await ui.find(exactly('WAITING'))
  expect(waiting?.props.dimColor).toBe(true)
  expect(waiting?.props.color).toBeUndefined()
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

test('the Today tab draws the TUI pane\'s compact rows: lead, title, right mark', async ($, on) => {
  const entry = (title: string, occurrence: Record<string, unknown>, extra: Record<string, unknown> = {}) => ({
    schema: 'agd.agenda-entry/v1',
    item: item('TODO', title, extra),
    occurrence: { date: '2026-10-15', time: null, extra: '', days: 0, late: false, ...occurrence },
  })
  const late = [
    entry('Pay the rent', { type: 'deadline', extra: '1 d. ago:', days: 1, late: true }),
    entry('File the taxes', { type: 'past-scheduled', extra: 'Sched. 2x:', days: 2, late: true }),
  ]
  const entries = [
    entry('Call the bank', { type: 'scheduled', time: '10:00', extra: 'Scheduled:' }),
    entry('Review the PR', { type: 'scheduled', time: '15:00', extra: 'Scheduled:' }, { priority: 'A' }),
    entry('Water plants', { type: 'scheduled', extra: 'Scheduled:' }, {
      planning: { scheduled: { date: '2026-10-15', repeater: '+3d' } },
    }),
    entry('Renew the lease', { type: 'deadline', extra: 'Deadline:' }),
    entry('Quarterly report', { type: 'upcoming-deadline', extra: 'In   3 d.:', days: -3 }),
    late[1],
    late[0],
    entry('Stand-up', { type: 'scheduled', time: '09:00', extra: 'Scheduled:' }, { todo: 'DONE', done: true }),
  ]
  panes(on)
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --status --json': { stdout: statusJson({ today: 4 }, late) },
    'agenda --views --json': { stdout: viewsJson([{ key: 'n', desc: 'Next', start: false }]) },
    'agenda --span day --json': { stdout: JSON.stringify(entries) },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: { ...PANE, bodyColumns: 44 } })
  // Section headers with the status band's counts, as in the TUI.
  expect((await ui.find(exactly('OVERDUE')))?.props.color).toBe('#f07178')
  expect(await ui.find(exactly('  2'))).toBeDefined()
  expect((await ui.find(exactly('TODAY')))?.props.color).toBe('#6ea8fe')
  expect(await ui.find(exactly('  4'))).toBeDefined()
  // No org-agenda prefix: the six-cell lead says when.
  expect(await ui.find(exactly('tasks:      '))).toBeUndefined()
  const badge = await ui.find(exactly('   2d '))
  expect(badge?.props.color).toBe('#f07178')
  expect(badge?.props.backgroundColor).toBe('#20252e')
  expect((await ui.find(exactly('  -1d ')))?.props.color).toBe('#f07178')
  expect((await ui.find(exactly('in 3d ')))?.props.color).toBe('#e5b567')
  const due = await ui.find(exactly('due   '))
  expect(due?.props.color).toBe('#e5b567')
  expect(due?.props.bold).toBe(true)
  // At 12:00 the 10:00 task is past and muted, 15:00 is next: ▸ and bold.
  expect((await ui.find(exactly('10:00 ')))?.props.color).toBe('#808896')
  const next = await ui.find(exactly('15:00 '))
  expect(next?.props.color).toBe('#6ea8fe')
  expect(next?.props.bold).toBe(true)
  expect((await ui.find(exactly('▸ ')))?.props.color).toBe('#6ea8fe')
  expect((await ui.find(exactly('Review the PR')))?.props.bold).toBe(true)
  // The right mark: priority, ↻ for a repeat, ✓ for done.
  expect((await ui.find(exactly(' A ')))?.props.color).toBe('#e5b567')
  expect(await ui.find(exactly('↻'))).toBeDefined()
  expect((await ui.find(exactly('✓')))?.props.color).toBe('#7ec68f')
  expect((await ui.find(exactly('Stand-up')))?.props.strikethrough).toBe(true)
  // Overdue rows come first, in the order agd gives them, each once.
  const titles = (await ui.findAll({ type: 'Text', text: /^(File the taxes|Pay the rent|Call the bank)$/ })).map((found) => found.text)
  expect(titles).toEqual(['Pay the rent', 'File the taxes', 'Call the bank'])
  await ui.unmount()
})

test('a saved view keeps org-agenda\'s prefix under the TUI\'s day lines', async ($, on) => {
  const section = {
    header: '',
    from: '2026-10-14',
    to: '2026-10-15',
    entries: [
      {
        schema: 'agd.agenda-entry/v1',
        item: item('TODO', 'File the taxes', { category: 'home' }),
        occurrence: { type: 'past-scheduled', date: '2026-10-15', time: null, extra: 'Sched. 2x:', days: 2, late: true },
      },
      {
        schema: 'agd.agenda-entry/v1',
        item: item('TODO', 'Call the bank'),
        occurrence: { type: 'scheduled', date: '2026-10-14', time: '10:00', extra: 'Scheduled:', days: 0, late: false },
      },
    ],
    items: [],
    groups: [],
    color: 'blue',
  }
  panes(on)
  agd(on, {
    'agenda --status': { stdout: '' },
    'agenda --views --json': { stdout: viewsJson([{ key: 'w', desc: 'Week', start: true }]) },
    'agenda --view w --json': { stdout: JSON.stringify([section]) },
  })
  await $.command.run(typed('agd'))
  const ui = await $.ui.mount({ plugin: 'agd', surface: 'terminal', component: 'Pane', requestId: 'agd', props: PANE })
  expect((await ui.find(exactly('home:       ')))?.props.color).toBe('#808896')
  expect((await ui.find(exactly('Sched. 2x:  ')))?.props.color).toBe('#f07178')
  expect(await ui.find(exactly('10:00...... '))).toBeDefined()
  // Day lines as org-agenda and the TUI head them; today's in accent.
  expect((await ui.find(exactly('Wednesday  14 October 2026')))?.props.color).toBe('#dee2e8')
  const today = await ui.find(exactly('Thursday   15 October 2026'))
  expect(today?.props.color).toBe('#6ea8fe')
  expect(today?.props.underline).toBe(true)
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
      'agenda --status --json': { stdout: statusJson() },
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
    'agenda --status --json': { stdout: statusJson({ today: 2, inbox: 5 }) },
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

test('/capture runs agd capture and says what it wrote as agd does', async ($, on) => {
  mock.clock(on, { now: NOON })
  const calls: string[] = []
  const mutation = {
    schema: 'agd.mutation/v1',
    item: item('TODO', 'buy milk', {
      file: '/home/me/org/inbox.org',
      line: 7,
      planning: { deadline: { date: '2026-10-16', time: null }, scheduled: { date: '2027-01-04', time: '09:00' } },
    }),
    location: 'inbox.org:7',
  }
  agd(
    on,
    {
      'agenda --status': { stdout: '1 today · 1 inbox\n' },
      'capture --json -- t buy milk due:fri': { stdout: JSON.stringify(mutation) },
      'capture --json -- t -1 point': {
        stdout: JSON.stringify({ ...mutation, item: item('TODO', '-1 point'), location: 'projects/tasks.org:1' }),
      },
    },
    calls,
  )
  const captured = await $.command.run(typed('capture', 't buy milk due:fri'))
  expect(captured.text).toBe(
    'inbox.org:7  captured · buy milk · scheduled Mon 4 Jan 2027 09:00 · deadline Fri 16 Oct',
  )
  expect(calls).toContain('capture --json -- t buy milk due:fri')
  // After `--` a text that starts with `-` is text, not an option.
  // The file is named under its root, as agd's terminal names it.
  expect((await $.command.run(typed('capture', 't -1 point'))).text).toBe('projects/tasks.org:1  captured · -1 point')
})

test('/capture reports agd\'s own error as a sentence, and its usage', async ($, on) => {
  const refused = {
    schema: 'agd.error/v1',
    code: 'unknown_template',
    message: "capture template 'zz' is not configured",
  }
  const stale = { schema: 'agd.error/v1', code: 'stale_ref', message: 'ref /org/a.org:3#sha256:0 is stale' }
  agd(on, {
    'agenda --status': { stdout: '' },
    // A skipped file is a warning line before agd's error.
    'capture --json -- zz nothing': {
      exitCode: 1,
      stderr: 'warning: skipped /org/bad.org: not UTF-8\n' + JSON.stringify(refused) + '\n',
    },
    'capture --json -- t stale': { exitCode: 1, stderr: JSON.stringify(stale) + '\n' },
  })
  expect((await $.command.run(typed('capture', 'zz nothing'))).text).toBe(
    "capture template 'zz' is not configured",
  )
  // The same sentence rule as the TUI's: a stale ref is said plainly.
  expect((await $.command.run(typed('capture', 't stale'))).text).toBe(
    'this heading changed on disk since agd read it',
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

test('show_band off hides the band and reads no counts', { options: { show_band: false } }, async ($, on) => {
  panes(on)
  on('ui.render', { component: 'AbovePrompt' }, async ($, e) => $.ui.resolve(e).Box({ children: [] }))
  const calls: string[] = []
  agd(
    on,
    {
      'agenda --status': { stdout: '2 today\n' },
      'agenda --views --json': { stdout: viewsJson([]) },
      'agenda --status --json': { stdout: statusJson({ today: 2 }) },
      'agenda --span day --json': { stdout: '[]' },
    },
    calls,
  )
  await $.command.run(typed('agd'))
  expect(await bandLine($)).toBeUndefined()
  expect(calls).not.toContain('agenda --status')
})
