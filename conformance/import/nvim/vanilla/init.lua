local root = vim.env.AGENDUM_FIXTURE_DIR
local opts = {
  org_agenda_files = { root .. '/tasks.org' },
  org_todo_keywords = { { 'TODO(t)', 'NEXT(n!)', '|', 'DONE(d!)' } },
  org_tag_alist = { 'work', 'home' },
  org_tag_persistent_alist = { 'important' },
  org_tags_exclude_from_inheritance = { 'noinherit' },
  org_highest_priority = 'A',
  org_lowest_priority = 'D',
  org_default_priority = 'C',
  org_deadline_warning_days = 6,
  org_agenda_span = 'week',
  org_agenda_start_day = '-1d',
  org_agenda_start_on_weekday = 0,
  org_agenda_skip_scheduled_if_done = true,
  org_agenda_skip_deadline_if_done = true,
  org_agenda_skip_timestamp_if_done = false,
  org_agenda_sorting_strategy = { 'priority-down', 'time-up' },
  org_agenda_time_grid = { '08:00', '12:00', '18:00' },
  org_log_done = 'time',
  org_log_repeat = 'note',
  org_log_into_drawer = true,
  org_capture_templates = {
    t = { description = 'Todo', target = root .. '/tasks.org', template = '* TODO %?' },
    x = { description = 'Dynamic', target = function() end, template = function() end },
  },
  org_agenda_custom_commands = {
    w = { description = 'Work', type = 'tags-todo', match = '+work/TODO|NEXT' },
  },
}
local files = {}
function files:load_sync() return self end
function files:filenames() return opts.org_agenda_files end
local module = {
  version = '0.7.5',
  is_setup_called = function() return true end,
  instance = function() return { files = files } end,
}
package.preload['orgmode'] = function() return module end
package.preload['orgmode.config'] = function() return { opts = opts } end
