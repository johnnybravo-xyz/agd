-- A lazy.nvim spec: opts are passed to require('orgmode').setup by config.
local root = vim.env.AGD_FIXTURE_DIR
return {
  'nvim-orgmode/orgmode',
  event = 'VeryLazy',
  ft = { 'org' },
  dependencies = { 'nvim-orgmode/org-bullets.nvim' },
  opts = {
    org_agenda_files = root .. '/**/*.org',
    org_default_notes_file = root .. '/inbox.org',
    org_todo_keywords = { 'TODO', 'WAIT', '|', 'DONE' },
    org_capture_templates = {
      n = { description = 'Note', template = '* %?\n%U', target = root .. '/inbox.org', headline = 'Notes' },
    },
  },
  config = function(_, opts)
    require('orgmode').setup(opts)
  end,
}
