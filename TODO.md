# TODO

Here you can find the current TODOs for the project. The TODOs are approximately listed by priority.

Each TODO is somewhat abstract and may require a lot of work to implement.
  
- Narrow `ShellContext` so screens only receive the shell/runtime capabilities they actually use.
- Refactor the call screen internals, especially the large `view.rs` and `workers.rs` modules.
- Revisit `ui/app/handlers` and consolidate routing/dispatch further if it keeps growing noisier.
- Add more focused lifecycle/startup/retry sequencing tests now that the app event/command boundary is stable.
- Audio capture, streaming and playback.
