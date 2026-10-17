-- Repro for bd-lt7rgi90: pandoc.utils.stringify(pandoc.Nodes(...)) is "".
-- Built from strings/elements so it runs on main without PR #859
-- (on main, pandoc.Nodes(42) still errors with "got number").
local s = pandoc.utils.stringify
local function show(label, v)
  io.stderr:write(string.format("%-34s -> %q\n", label, s(v)))
end
show("Nodes('42')", pandoc.Nodes('42'))
show("Nodes({'a','2'})", pandoc.Nodes({ 'a', '2' }))
show("Nodes(pandoc.Para{'p'})", pandoc.Nodes(pandoc.Para { pandoc.Str('p') }))
show("Nodes()  (empty)", pandoc.Nodes({}))
show("Span(Nodes('42')) [workaround]", pandoc.Span(pandoc.Nodes('42')))
-- Sibling pandoc.utils dispatchers on the same value:
io.stderr:write(string.format("%-34s -> %s\n", "type(Nodes('x'))", pandoc.utils.type(pandoc.Nodes('x'))))
io.stderr:write(string.format("%-34s -> %d inlines\n", "blocks_to_inlines(Nodes(Para))",
  #pandoc.utils.blocks_to_inlines(pandoc.Nodes(pandoc.Para { pandoc.Str('p') }))))
io.stderr:write(string.format("%-34s -> %s\n", "equals(Nodes('x'), Nodes('x'))",
  tostring(pandoc.utils.equals(pandoc.Nodes('x'), pandoc.Nodes('x')))))
return {}
