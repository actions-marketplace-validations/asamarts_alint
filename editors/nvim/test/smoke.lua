-- In-Neovim assertion for the alint LSP smoke (see run-smoke.sh).
-- Waits for the alint server to attach to the current buffer and
-- publish diagnostics, then asserts at least one is sourced from alint.
-- Exits the process 0 on success, 1 on failure.

local got = vim.wait(20000, function()
  return #vim.diagnostic.get(0) > 0
end, 200)

local diags = vim.diagnostic.get(0)
local clients = vim.lsp.get_clients({ bufnr = 0 })
io.stderr:write(("nvim smoke: attached_clients=%d diagnostics=%d\n"):format(#clients, #diags))

if not got or #diags == 0 then
  io.stderr:write("nvim smoke FAIL: no diagnostics from the alint server within timeout\n")
  os.exit(1)
end

local first = diags[1]
local source = first.source or "?"
io.stderr:write(("nvim smoke: source=%s message=%q\n"):format(source, first.message or ""))
if source ~= "alint" then
  io.stderr:write("nvim smoke FAIL: diagnostic source is not 'alint'\n")
  os.exit(1)
end

io.stderr:write("nvim smoke PASS\n")
os.exit(0)
