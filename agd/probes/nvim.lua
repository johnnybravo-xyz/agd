local function fail(message)
  error("agd nvim import: " .. message, 0)
end

local ok_orgmode, orgmode = pcall(require, "orgmode")
if not ok_orgmode then
  fail("require('orgmode') failed")
end
if type(orgmode.is_setup_called) ~= "function" or not orgmode.is_setup_called() then
  fail("nvim-orgmode setup has not completed")
end
local ok_config, config = pcall(require, "orgmode.config")
if not ok_config or type(config) ~= "table" then
  fail("require('orgmode.config') returned an unsupported value")
end
local opts = config.opts or config
if type(opts) ~= "table" then
  fail("nvim-orgmode config has no opts table")
end
if type(orgmode.instance) ~= "function" then
  fail("nvim-orgmode instance() capability is unavailable")
end
local instance = orgmode.instance()
if type(instance) ~= "table" or type(instance.files) ~= "table" then
  fail("nvim-orgmode files capability is unavailable")
end
if type(instance.files.load_sync) ~= "function" then
  fail("nvim-orgmode files:load_sync capability is unavailable")
end
local ok_loaded, loaded = pcall(instance.files.load_sync, instance.files, true, 5000)
if not ok_loaded then
  fail("nvim-orgmode file loading failed: " .. tostring(loaded))
end
if type(loaded) ~= "table" or type(loaded.filenames) ~= "function" then
  fail("nvim-orgmode resolved filenames capability is unavailable")
end
local ok_filenames, filenames = pcall(loaded.filenames, loaded)
if not ok_filenames then
  fail("nvim-orgmode filename resolution failed: " .. tostring(filenames))
end
if type(filenames) ~= "table" then
  fail("nvim-orgmode filename resolution returned an unsupported value")
end

local org_version = orgmode.version or opts.version
if org_version == nil then
  local module_path = package.searchpath("orgmode", package.path)
    or vim.api.nvim_get_runtime_file("lua/orgmode/init.lua", false)[1]
  local root = module_path and module_path:gsub("/lua/orgmode/init%.lua$", "") or nil
  if root ~= nil and root ~= module_path and type(vim.system) == "function" then
    local result = vim.system({ "git", "-C", root, "rev-parse", "HEAD" }, { text = true }):wait()
    local commit = result.code == 0 and vim.trim(result.stdout or "") or ""
    if commit ~= "" then
      org_version = "git@" .. commit
    end
  end
end
org_version = org_version or "unknown"

local function strings(values)
  local result = {}
  for _, value in ipairs(values or {}) do
    result[#result + 1] = tostring(value)
  end
  return result
end

local function todo_sequences(values)
  local sequences = {}
  if type(values) == "table" and type(values[1]) == "table" then
    for _, sequence in ipairs(values) do
      sequences[#sequences + 1] = strings(sequence)
    end
  else
    sequences[1] = strings(values or { "TODO", "|", "DONE" })
  end
  return sequences
end

local function capture_templates(values)
  local result = {}
  for key, template in pairs(values or {}) do
    if type(template) == "table" then
      local body = template.template or template[3] or ""
      local target = template.target or template[2]
      local enabled = type(body) == "string" and type(target) ~= "function"
      local diagnostics = {}
      if type(body) ~= "string" then
        diagnostics[#diagnostics + 1] = "dynamic template body is unsupported"
      elseif body:find("%(", 1, true) then
        enabled = false
        diagnostics[#diagnostics + 1] = "%(sexp) template forms are disabled"
      end
      if type(target) == "function" then
        diagnostics[#diagnostics + 1] = "dynamic capture target is unsupported"
      end
      result[tostring(key)] = {
        desc = tostring(template.description or template[1] or key),
        file = type(target) == "string" and target or nil,
        template = type(body) == "string" and body or "",
        enabled = enabled,
        diagnostics = diagnostics,
      }
    end
  end
  return result
end

-- nvim-orgmode custom commands -> views:
-- { w = { description = "...", types = { { type = "tags_todo", match = "...",
--   org_agenda_span = "day", org_agenda_overriding_header = "..." } } } }
local view_kinds = { agenda = "agenda", tags = "tags", tags_todo = "tags-todo" }
local view_options = { type = true, match = true, org_agenda_span = true, org_agenda_overriding_header = true }

local function custom_commands(values)
  local result = {}
  for key, command in pairs(values or {}) do
    local blocks, notes, supported = {}, {}, type(command) == "table" and type(command.types) == "table"
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
        for option in pairs(spec) do
          if not view_options[option] then
            notes[#notes + 1] = "option " .. tostring(option) .. " is not imported"
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
      diagnostics = supported and notes or { "custom command form is unsupported" },
    }
  end
  table.sort(result, function(left, right) return left.key < right.key end)
  return result
end

local sorting = opts.org_agenda_sorting_strategy or { agenda = { "time-up", "priority-down", "category-keep" } }
local version = vim.version()
local week_start = opts.org_agenda_start_on_weekday == 0 and "sunday" or "monday"
local output = {
  editor = "nvim",
  editor_version = string.format("%d.%d.%d", version.major, version.minor, version.patch),
  org_version = tostring(org_version),
  source = vim.env.AGD_IMPORT_SOURCE or "profile",
  agenda_files = strings(filenames),
  todo_sequences = todo_sequences(opts.org_todo_keywords),
  tag_alist = strings(opts.org_tag_alist),
  persistent_tags = strings(opts.org_tag_persistent_alist),
  tag_inheritance_exclusions = strings(opts.org_tags_exclude_from_inheritance),
  highest_priority = tostring(opts.org_highest_priority or "A"),
  lowest_priority = tostring(opts.org_lowest_priority or "C"),
  default_priority = tostring(opts.org_default_priority or "B"),
  deadline_warning_days = tonumber(opts.org_deadline_warning_days or 14),
  agenda_span = tostring(opts.org_agenda_span or "week"),
  agenda_start_day = opts.org_agenda_start_day,
  week_start = week_start,
  skip_scheduled_if_done = opts.org_agenda_skip_scheduled_if_done == true,
  skip_deadline_if_done = opts.org_agenda_skip_deadline_if_done == true,
  skip_timestamp_if_done = opts.org_agenda_skip_timestamp_if_done == true,
  sorting_strategy = strings(sorting.agenda or sorting),
  log_done = tostring(opts.org_log_done or "nil"),
  log_repeat = tostring(opts.org_log_repeat or "nil"),
  -- nvim-orgmode has no reschedule/redeadline logging.
  log_reschedule = "nil",
  log_redeadline = "nil",
  log_into_drawer = opts.org_log_into_drawer == true,
  capture = capture_templates(opts.org_capture_templates),
  groups = {},
  commands = custom_commands(opts.org_agenda_custom_commands),
  diagnostics = {},
}
vim.api.nvim_out_write("AGD_JSON:" .. vim.json.encode(output) .. "\n")
