-- nvim-orgmode configured with a plain setup() call.
local root = vim.env.AGD_FIXTURE_DIR
vim.opt.number = true
require('orgmode').setup({
  org_agenda_files = { root .. '/tasks.org' },
  org_default_notes_file = root .. '/tasks.org',
  org_todo_keywords = { 'TODO(t)', 'NEXT(n!)', '|', 'DONE(d!)' },
  org_tags_exclude_from_inheritance = { 'noinherit' },
  org_priority_highest = 'A',
  org_priority_lowest = 'D',
  org_priority_default = 'C',
  org_deadline_warning_days = 6,
  org_agenda_span = 'week',
  org_agenda_start_day = '-1d',
  org_agenda_start_on_weekday = 0,
  org_agenda_skip_scheduled_if_done = true,
  org_agenda_skip_deadline_if_done = true,
  org_agenda_sorting_strategy = { agenda = { 'priority-down', 'time-up' } },
  org_log_done = 'time',
  org_log_repeat = 'note',
  org_log_into_drawer = 'LOGBOOK',
  org_capture_templates = {
    t = { description = 'Todo', template = '* TODO %?' },
    x = { description = 'Dynamic', target = function() end, template = '* TODO %?' },
  },
  org_agenda_custom_commands = {
    w = { description = 'Work', types = { { type = 'tags_todo', match = '+work/TODO|NEXT' } } },
    n = { description = 'Next', types = { { type = 'tags_todo', match = 'TODO="NEXT"' } } },
    r = { description = 'Review', types = {
      { type = 'agenda', org_agenda_span = 'day', org_agenda_overriding_header = 'Today' },
      { type = 'tags', match = 'review' },
    } },
  },
})
-- A plugin that is not installed in `nvim --clean` must not stop the read.
require('org-bullets').setup()
