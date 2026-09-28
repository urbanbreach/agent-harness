set pagination off
set confirm off
set debuginfod enabled off
set breakpoint pending on
set logging file allocation-stacks.log
set logging redirect on
set logging overwrite on
set logging enabled on
break malloc
commands
silent
bt 30
ignore 1 199
continue
end
run
