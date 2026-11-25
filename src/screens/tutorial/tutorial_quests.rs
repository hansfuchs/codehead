use super::{Quest, Task};

pub(super) fn make_quests(crew_tasks: Vec<Task>) -> Vec<Quest> {
    vec![
        quest_task(),
        quest_crew(crew_tasks),
        quest_changes(),
        quest_subthread(),
        quest_merge(),
        quest_threads(),
        quest_flow(),
        quest_finish(),
    ]
}

fn quest_task() -> Quest {
    Quest {
        title: "start a task",
        tasks: vec![Task::Send, Task::Attach],
        about: &[
            "1 thread = 1 task: its messages, tool runs and changes.",
            "the agents run in the background: close codehead, they go on.",
            "paste or drop a file into a message: your agents read it.",
        ],
        doc: "concepts/threads/",
        advanced: false,
    }
}

fn quest_crew(crew_tasks: Vec<Task>) -> Quest {
    Quest {
        title: "work with the crew",
        tasks: crew_tasks,
        about: &[
            "@name: a message to 1 agent. agents talk to each other with send.",
            "#name: a shortcut, a text you wrote once (settings → shortcuts).",
            "use shortcuts for what you type again and again: review, plan, ship.",
        ],
        doc: "concepts/messaging/",
        advanced: false,
    }
}

fn quest_changes() -> Quest {
    Quest {
        title: "check and commit",
        tasks: vec![Task::Changes, Task::Ide, Task::Stage, Task::Commit],
        about: &[
            "4 views per thread: chat F1 · changes F2 · merge F3 · files F4.",
            "changes: the diff of the checkout. comment, stage, commit, push.",
            "e: the line under the cursor in your IDE.",
            "threads in 1 checkout share it: commit only your task's files.",
        ],
        doc: "concepts/thread-view/",
        advanced: false,
    }
}

fn quest_subthread() -> Quest {
    Quest {
        title: "side questions",
        tasks: vec![Task::Sub, Task::SubDone],
        about: &[
            ">sub @name task: a sub-thread does it in the same checkout.",
            "this thread waits, then gets the result: 0 noise in this chat.",
            "use it for a detour or a check. a new task: a new thread.",
        ],
        doc: "concepts/threads/",
        advanced: true,
    }
}

fn quest_merge() -> Quest {
    Quest {
        title: "ship it",
        tasks: vec![Task::AskPr, Task::MergeView, Task::Merged],
        about: &[
            "merge F3: the thread's pull requests (its branch, links in its log).",
            "checks, reviews, comments, labels and the merge: 0 browser.",
            "here: a mock PR, 0 GitHub calls.",
        ],
        doc: "reference/keys/#merge-view",
        advanced: true,
    }
}

fn quest_threads() -> Quest {
    Quest {
        title: "many threads",
        tasks: vec![Task::Menu, Task::Switch],
        about: &[
            "1 thread per task, as many as you like, all at work at once.",
            "ctrl+p: the menu. every thread and every action, fuzzy search.",
            "a thread that needs you shows it in the list.",
        ],
        doc: "concepts/threads/",
        advanced: true,
    }
}

fn quest_flow() -> Quest {
    Quest {
        title: "new thread, with a flow",
        tasks: vec![Task::Menu2, Task::New, Task::Flow, Task::Start, Task::FlowRun],
        about: &[
            "a flow: the steps an agent takes for a kind of task, 1 at a time.",
            "gates wait for your ok. loops go back until a condition holds.",
            "use one for repeat work: feature, bug fix, review. skip it for a quick question.",
            "home repo: 1 git repo for crew, notes, settings and repos, on every device.",
            "notes: the new tab attaches them. ask your agents to write them: 0 fixed format.",
        ],
        doc: "concepts/flows/",
        advanced: true,
    }
}

fn quest_finish() -> Quest {
    Quest {
        title: "finish",
        tasks: vec![Task::Leave],
        about: &[
            "keys to keep: tab focus · ctrl+. actions · ctrl+p menu · F1-F4 views · @ agents · # shortcuts · esc esc back.",
        ],
        doc: "reference/keys/",
        advanced: true,
    }
}
