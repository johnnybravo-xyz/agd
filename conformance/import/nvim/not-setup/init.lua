package.preload['orgmode'] = function()
  return {
    is_setup_called = function() return false end,
  }
end
package.preload['orgmode.config'] = function() return { opts = {} } end
