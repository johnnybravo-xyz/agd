-- agd's Neovim config reader for nvim-orgmode. Runs in `nvim --clean
-- --headless -l` with AGD_IMPORT_FILES (newline-separated) naming the config
-- files to run and AGD_IMPORT_ROOT the config directory.
--
-- Only those files run. require("orgmode") is a stand-in that records the
-- options passed to setup(); lazy.nvim specs for the plugin (returned by a
-- file, or passed to require("lazy").setup) have their opts and config run
-- the way lazy.nvim would; any other module that is not installed is a
-- harmless stand-in. Neither the plugin nor a plugin manager is needed.
-- Prints AGD_JSON:<payload> on stdout.

local files = vim.split(vim.env.AGD_IMPORT_FILES or "", "\n", { trimempty = true })
local root = vim.env.AGD_IMPORT_ROOT
if root and root ~= "" then
  package.path = root .. "/lua/?.lua;" .. root .. "/lua/?/init.lua;" .. package.path
end

local diagnostics = {}
local function note(status, key, message)
  diagnostics[#diagnostics + 1] = { status = status, key = key, message = message }
end

local current_file
local function name_of(file)
  return vim.fn.fnamemodify(file, ":t")
end

-- A module that is not installed: every field and every call is another
-- stand-in, so `require("org-bullets").setup({})` runs and does nothing.
local function stand_in()
  return setmetatable({}, {
    __index = function()
      return stand_in()
    end,
    __call = function()
      return stand_in()
    end,
  })
end

local captured, captured_in
local orgmode = setmetatable({
  setup = function(opts)
    captured = opts or {}
    captured_in = current_file
    return stand_in()
  end,
}, {
  __index = function()
    return stand_in()
  end,
})
package.preload["orgmode"] = function()
  return orgmode
end

local function is_orgmode_spec(spec)
  return type(spec) == "table"
    and ((type(spec[1]) == "string" and spec[1]:match("orgmode$") ~= nil) or spec.name == "orgmode")
end

-- lazy.nvim semantics: opts may be a function of (plugin, opts); config may
-- be a function of (plugin, opts), or true, or absent with opts given, both
-- of which mean require("orgmode").setup(opts).
local function run_spec(spec)
  local opts = spec.opts
  if type(opts) == "function" then
    local ok, value = pcall(opts, spec, {})
    opts = ok and value or {}
  end
  if type(spec.config) == "function" then
    local ok, err = pcall(spec.config, spec, opts or {})
    if not ok then
      note("unsupported", name_of(current_file), "orgmode config() failed: " .. tostring(err))
    end
  elseif spec.config == true or opts ~= nil then
    orgmode.setup(opts or {})
  end
end

local function run_specs(value)
  if type(value) ~= "table" then
    return
  end
  if is_orgmode_spec(value) then
    run_spec(value)
  end
  for _, item in ipairs(value) do
    run_specs(item)
  end
  run_specs(value.dependencies)
  run_specs(value.spec)
end

package.preload["lazy"] = function()
  return setmetatable({
    setup = function(spec, opts)
      run_specs(spec)
      run_specs(opts)
    end,
  }, {
    __index = function()
      return stand_in()
    end,
  })
end

-- Tried last, after the config's own lua/ modules and Neovim's runtime.
table.insert(package.loaders, function(module)
  return function()
    return stand_in()
  end, module
end)

for _, file in ipairs(files) do
  current_file = file
  local ok, result = pcall(dofile, file)
  if ok then
    run_specs(result)
  else
    note("unsupported", name_of(file), tostring(result))
  end
end

if not captured then
  local names = vim.tbl_map(name_of, files)
  error("agd nvim import: no require('orgmode').setup() call found in " .. table.concat(names, ", "), 0)
end
note("exact", "setup", name_of(captured_in))

-- nvim-orgmode's defaults for the options agd imports (config/defaults.lua).
local defaults = {
  org_todo_keywords = { "TODO", "|", "DONE" },
  org_deadline_warning_days = 14,
  org_agenda_span = "week",
  org_agenda_start_on_weekday = 1,
  org_priority_highest = "A",
  org_priority_default = "B",
  org_priority_lowest = "C",
  org_tags_column = -80,
  org_log_done = "time",
  org_log_repeat = "time",
  org_agenda_sorting_strategy = { agenda = { "time-up", "priority-down", "category-keep" } },
  org_capture_templates = { t = { description = "Task", template = "* TODO %?\n  %u" } },
}
local function option(name)
  if captured[name] ~= nil then
    return captured[name]
  end
  return defaults[name]
end

local function strings(values)
  local result = {}
  for _, value in ipairs(type(values) == "table" and values or {}) do
    result[#result + 1] = tostring(value)
  end
  return result
end

-- `~` and environment variables only: vim.fn.expand would also expand
-- wildcards, freezing a glob into today's files.
local function expand(path)
  return type(path) == "string" and path ~= "" and vim.fs.normalize(path) or nil
end

local function todo_sequences(values)
  if type(values) == "table" and type(values[1]) == "table" then
    local sequences = {}
    for _, sequence in ipairs(values) do
      sequences[#sequences + 1] = strings(sequence)
    end
    return sequences
  end
  return { strings(values) }
end

local function agenda_files(value)
  if type(value) == "string" then
    value = value ~= "" and { value } or {}
  end
  local result = {}
  for _, entry in ipairs(type(value) == "table" and value or {}) do
    result[#result + 1] = expand(entry)
  end
  return result
end

local function capture_templates(values)
  local result = {}
  local notes_file = expand(captured.org_default_notes_file)
  -- A Lua table keyed by template key keeps no written order, so templates
  -- are listed by key; config.toml then holds that order for the user to change.
  local keys = {}
  for key in pairs(values or {}) do
    keys[#keys + 1] = key
  end
  table.sort(keys, function(left, right)
    return tostring(left) < tostring(right)
  end)
  for _, key in ipairs(keys) do
    local template = values[key]
    if type(template) == "table" then
      local body = template.template or ""
      local target = template.target or notes_file
      local enabled, reasons = true, {}
      if type(body) ~= "string" then
        enabled = false
        reasons[#reasons + 1] = "a computed template body is unsupported"
      elseif body:find("%(", 1, true) then
        enabled = false
        reasons[#reasons + 1] = "%(sexp) template forms are disabled"
      end
      if type(target) == "function" then
        enabled = false
        reasons[#reasons + 1] = "a computed capture target is unsupported"
      elseif type(target) ~= "string" then
        enabled = false
        reasons[#reasons + 1] = "no target and no org_default_notes_file"
      end
      if template.datetree then
        enabled = false
        reasons[#reasons + 1] = "datetree targets are not imported"
      end
      result[#result + 1] = {
        key = tostring(key),
        desc = tostring(template.description or key),
        file = type(target) == "string" and expand(target) or nil,
        heading = type(template.headline) == "string" and template.headline or nil,
        template = type(body) == "string" and body or "",
        enabled = enabled,
        diagnostics = reasons,
      }
    end
  end
  return result
end

-- org_agenda_custom_commands -> views:
-- { w = { description = "...", types = { { type = "tags_todo", match = "...",
--   org_agenda_span = "day", org_agenda_overriding_header = "..." } } } }
local view_kinds = { agenda = "agenda", tags = "tags", tags_todo = "tags-todo" }
local view_options = { type = true, match = true, org_agenda_span = true, org_agenda_overriding_header = true }

local function custom_commands(values)
  local result = {}
  for key, command in pairs(values or {}) do
    local blocks, notes = {}, {}
    local supported = type(command) == "table" and type(command.types) == "table"
    for _, spec in ipairs(supported and command.types or {}) do
      local kind = type(spec) == "table" and view_kinds[spec.type] or nil
      if not kind then
        supported = false
      else
        local span = spec.org_agenda_span
        blocks[#blocks + 1] = {
          kind = kind,
          match = type(spec.match) == "string" and spec.match ~= "" and spec.match or nil,
          span = (type(span) == "string" or type(span) == "number") and tostring(span) or nil,
          header = type(spec.org_agenda_overriding_header) == "string" and spec.org_agenda_overriding_header or nil,
        }
        for name in pairs(spec) do
          if not view_options[name] then
            notes[#notes + 1] = "option " .. tostring(name) .. " is not imported"
          end
        end
      end
    end
    table.sort(notes)
    supported = supported and #blocks > 0
    result[#result + 1] = {
      key = tostring(key),
      description = tostring(type(command) == "table" and (command.description or key) or key),
      enabled = supported,
      blocks = supported and blocks or {},
      diagnostics = supported and vim.fn.uniq(notes) or { "custom command form is unsupported" },
    }
  end
  table.sort(result, function(left, right)
    return left.key < right.key
  end)
  return result
end

local sorting = option("org_agenda_sorting_strategy")
local version = vim.version()
local output = {
  editor = "nvim",
  editor_version = string.format("%d.%d.%d", version.major, version.minor, version.patch),
  agenda_files = agenda_files(captured.org_agenda_files),
  todo_sequences = todo_sequences(option("org_todo_keywords")),
  tag_alist = {},
  persistent_tags = {},
  tags_column = tonumber(option("org_tags_column")),
  tag_inheritance_exclusions = strings(captured.org_tags_exclude_from_inheritance),
  highest_priority = tostring(option("org_priority_highest")),
  lowest_priority = tostring(option("org_priority_lowest")),
  default_priority = tostring(option("org_priority_default")),
  deadline_warning_days = tonumber(option("org_deadline_warning_days")),
  agenda_span = tostring(option("org_agenda_span")),
  agenda_start_day = captured.org_agenda_start_day,
  week_start = option("org_agenda_start_on_weekday") == 0 and "sunday" or "monday",
  skip_scheduled_if_done = captured.org_agenda_skip_scheduled_if_done == true,
  skip_deadline_if_done = captured.org_agenda_skip_deadline_if_done == true,
  -- nvim-orgmode has no timestamp skipping, nor reschedule/redeadline logging.
  skip_timestamp_if_done = false,
  sorting_strategy = strings(sorting.agenda or sorting),
  log_done = tostring(option("org_log_done") or "nil"),
  log_repeat = tostring(option("org_log_repeat") or "nil"),
  log_reschedule = "nil",
  log_redeadline = "nil",
  log_into_drawer = type(captured.org_log_into_drawer) == "string" and captured.org_log_into_drawer ~= "",
  capture = capture_templates(option("org_capture_templates")),
  commands = custom_commands(captured.org_agenda_custom_commands),
  diagnostics = diagnostics,
}
io.stdout:write("AGD_JSON:" .. vim.json.encode(output) .. "\n")
