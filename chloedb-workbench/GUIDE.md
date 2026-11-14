# Workbench: a guide

Workbench runs several Claude Code agents side by side, each in its own
terminal and most often its own git worktree, and shows you what they are
doing at the level you want to read it: the goal, the questions waiting for
you, the story of one agent, the map of the code, the diff, or the terminal
itself. This page says what each screen is for and when to open it. The
README beside this guide is the reference for installing it and for its HTTP API.

> You can read a repository without starting any agent. Search, Map,
> Trace and Library each offer **Open a repository**: type the folder of a
> git checkout (Tab completes folders, git checkouts come first) and
> Workbench indexes it.

## The first ten minutes

1. **Start the server and open its link.** `chloedb-workbench serve` prints
   a URL with a token. Open it. The left rail lists the screens; the right
   rail, **Needs you**, lists what is waiting for you.
2. **Open your repository first, if you want to look before you change
   anything.** Press <kbd>/</kbd> to search, or open [Map](#/map) or
   [Library](#/library). Each one asks for the repository the first time.
   Search finds code, the docs of its libraries and past decisions; Map
   shows the files sized by lines; [Trace](#/trace) answers "when and why
   did this change" from git history.
3. **Start an agent.** Press <kbd>n</kbd> (New agent). Give the repository,
   a task in a sentence or two, a short name (`cents`, `paybutton`), and a
   model if you have one in mind. It starts in its own worktree on the
   branch `wb/<name>`, so it cannot step on your checkout or on other
   agents.
4. **Answer what it asks.** When it needs a permission or a decision, a
   card appears in **Needs you** and the [Decide](#/decide) badge counts it.
   Answer there rather than in the terminal: Decide shows what the agent
   wants to run and what was decided before.
5. **Follow it.** [Story](#/story) tells one agent's work as chapters, a
   chapter per turn. Pick the agent at the top. "Talk to it at this level"
   steers it without typing into the terminal.
6. **Check the result.** [Ship](#/ship) shows an agent's changes, what they
   touch and the draft of a pull request. If the agent started a web app,
   [Preview](#/preview) shows it.
7. **Several agents?** Set a goal on [Mission](#/mission) and see every
   outcome move from Next to Done.

## The screens

### Mission: what the fleet is for

The goal, its outcomes in four columns (Done, In motion, Needs you, Next),
one live sentence per agent, the questions waiting for you and what the
fleet learned today. Open it when you run several agents toward one end.

A goal can be staffed three ways (**Set a goal**, or **+ New goal** on a
goal):

| choice | what happens | when |
|---|---|---|
| A lead plans it | a lead agent reads the code, breaks the goal into 3 to 8 outcomes and starts an agent for each | you know the end, not the steps |
| I know the outcomes | you type the outcomes, one per line (after " — ", how to tell it is done); one agent starts per outcome, no planner | you already know the split, and don't want to pay a planner to find it again |
| Agents already running | you tick agents that are already working; each becomes an outcome | you started agents by hand and want to see them as one goal |

The lead's (or the agents') model and effort are picked in the same form.

### Decide: questions and rules

Every question an agent asked you (a permission, or `ask_human` with
options), with what ChloeCtx remembers about it. Answer in one click; tick
"Make it a rule" and every agent of the repository is told it.

**Rules** lists the repository's rules. Add one directly in the box at its
top (no question needed), edit one, turn it off, or delete it. An edited
rule is told again to every agent at its next step.

### Trace: when and why something changed

Ask in your own words: "cart total rounding from floor to half-up",
"`to_cents`", "money.py". Trace reads git on every branch (the agents'
`wb/*` ones too), Workbench's events and ChloeCtx's decisions, and lays out
where it happened, oldest first, with who decided it and why. Use it
before you change something that changed before.

### Map: who works where

The repository's tracked files, sized by lines, tinted by the agent
changing each one; collisions (two agents in one file, or one heading into
a file another changed) are hatched red, and ◆ marks the decisions ChloeCtx
can explain. Click a folder to go in, a file for who touched it, a ◆ for
why. Small files fold into "… n small" only in big folders; click the fold
to open it. **Heat** shows the files agents keep correcting.

### Story: one agent, at four levels

Intent (why it exists), Story (a chapter per turn), Steps (its tool calls,
grouped) and Terminal. <kbd>]</kbd> goes a level deeper, <kbd>[</kbd> back
up. The row at the top picks the agent. The side shows what ChloeCtx put
in its context and a box to steer it at the level you are reading.

### Preview: the app an agent runs

When an agent starts a dev server and sets its preview, the page shows
here. Click an element to ask for a change to it; turn the overlay off to
use the page as is. **Test with a persona** sends a simulated customer
through it.

### Ship: is this ready to merge

An agent's diff, its blast radius (the functions it changed, their callers
and tests), the decisions behind it and a pull request draft you can copy.

### Flow: where the time went

The last hour per agent: the model thinking, tools, tests, waiting on you,
redoing a failed step, idle. A coach suggests what to change, such as
allowing a command you approved many times.

### Health: tests and alerts

Rules written as SQL over Workbench's tables (a test failing twice in an
hour, a file three agents edited), the incidents they open, every test the
agents ran (flaky ones, slow ones) and the hotspots.

### Library: code and docs search

The repository's code, chunked and ranked in Workbench's own ChloeDB (no
model), as each agent's worktree sees it, and the docs of its libraries.
The same search the agents use.

### Away: what happened while you were gone

When you come back, a summary since you last looked: outcomes done,
questions decided and still open, a line per agent, and moments to replay.

### Terminals and Decisions

**Terminals** is the grid of every agent's live terminal; <kbd>z</kbd>
zooms one. The **Decisions** button in the top bar is the repository's
decisions on a timeline, one lane per agent, with **Ask why**.

### Search

<kbd>/</kbd> anywhere (outside a text box) opens one box over code, docs
and decisions. Enter opens a hit; <kbd>⇧</kbd><kbd>Enter</kbd> on a line of
code tells why it is like this.

## The keyboard

| keys | what |
|---|---|
| <kbd>n</kbd> | New agent |
| <kbd>z</kbd> | zoom the selected agent's terminal, or back to the grid |
| <kbd>/</kbd> | search |
| <kbd>?</kbd> | the list of keys |
| <kbd>[</kbd> <kbd>]</kbd> | up or down a level (Intent, Plan, Story, Changes, Terminal) |
| <kbd>j</kbd> <kbd>k</kbd> | the next or previous item on a screen |
| <kbd>1</kbd>…<kbd>9</kbd> | on Decide, pick an option |
| <kbd>Ctrl</kbd><kbd>b</kbd> then a key | the same commands from inside a terminal, as in tmux: <kbd>z</kbd> leaves a zoomed terminal, <kbd>x</kbd> stops an agent (asks first), <kbd>1</kbd>…<kbd>9</kbd> zooms agent 1 to 9, <kbd>d</kbd> opens Decisions |

Single keys act only outside a terminal or a text box: inside a terminal
every key goes to the agent (Claude Code uses <kbd>Esc</kbd> to interrupt).

## Words

- **Agent**: one Claude Code session Workbench started, with a name.
- **Worktree**: a second checkout of the repository on its own branch
  (`wb/<name>`), so agents don't edit the same files on disk.
- **Goal** and **outcome**: what you want, and the results that make it
  true, each with an agent.
- **Rule**: something you decided once, told to every agent of the
  repository.
- **ChloeCtx**: Workbench's memory: what was decided, by whom and why,
  across sessions.
