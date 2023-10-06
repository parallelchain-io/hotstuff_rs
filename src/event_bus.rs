use crate::events::*;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::thread::JoinHandle;

pub(crate) type HandlerPtr<T> = Box<dyn Fn(&T) + Send>;

pub(crate) struct EventHandlers {
    pub(crate) insert_block_handlers: Vec<HandlerPtr<InsertBlockEvent>>,
    pub(crate) commit_block_handlers: Vec<HandlerPtr<CommitBlockEvent>>,
    pub(crate) prune_block_handlers: Vec<HandlerPtr<PruneBlockEvent>>,
    pub(crate) propose_handlers: Vec<HandlerPtr<ProposeEvent>>,
    pub(crate) receive_proposal_handlers: Vec<HandlerPtr<ReceiveProposalEvent>>,
    pub(crate) nudge_handlers: Vec<HandlerPtr<NudgeEvent>>,
    pub(crate) receive_nudge_handlers: Vec<HandlerPtr<ReceiveNudgeEvent>>,
    pub(crate) vote_handlers: Vec<HandlerPtr<VoteEvent>>,
    pub(crate) receive_vote_handlers: Vec<HandlerPtr<ReceiveVoteEvent>>,
    pub(crate) new_view_handlers: Vec<HandlerPtr<NewViewEvent>>,
    pub(crate) receive_new_view_handlers: Vec<HandlerPtr<ReceiveNewViewEvent>>,
    pub(crate) start_view_handlers: Vec<HandlerPtr<StartViewEvent>>,
    pub(crate) view_timeout_handlers: Vec<HandlerPtr<ViewTimeoutEvent>>,
    pub(crate) start_sync_handlers: Vec<HandlerPtr<StartSyncEvent>>,
    pub(crate) end_sync_handlers: Vec<HandlerPtr<EndSyncEvent>>,
}

impl EventHandlers {

    pub fn fire_handlers(&self, event: Event) {
        match event {
            Event::InsertBlock(insert_block_event) => 
                self.insert_block_handlers.iter().for_each(|handler| handler(&insert_block_event)),

            Event::CommitBlock(commit_block_event) => 
                self.commit_block_handlers.iter().for_each(|handler| handler(&commit_block_event)),

            Event::PruneBlock(prune_block_event) => 
                self.prune_block_handlers.iter().for_each(|handler| handler(&prune_block_event)),

            Event::Propose(propose_event) => 
                self.propose_handlers.iter().for_each(|handler| handler(&propose_event)),

            Event::ReceiveProposal(receive_proposal_event) => 
                self.receive_proposal_handlers.iter().for_each(|handler| handler(&receive_proposal_event)),

            Event::Nudge(nudge_event) => 
                self.nudge_handlers.iter().for_each(|handler| handler(&nudge_event)),

            Event::ReceiveNudge(receive_nudge_event) => 
                self.receive_nudge_handlers.iter().for_each(|handler| handler(&receive_nudge_event)),

            Event::Vote(vote_event) => 
                self.vote_handlers.iter().for_each(|handler| handler(&vote_event)),

            Event::ReceiveVote(receive_vote_event) => 
                self.receive_vote_handlers.iter().for_each(|handler| handler(&receive_vote_event)),

            Event::NewView(new_view_event) => 
                self.new_view_handlers.iter().for_each(|handler| handler(&new_view_event)),

            Event::ReceiveNewView(receive_new_view) => 
                self.receive_new_view_handlers.iter().for_each(|handler| handler(&receive_new_view)),

            Event::StartView(start_view_event) => 
                self.start_view_handlers.iter().for_each(|handler| handler(&start_view_event)),

            Event::ViewTimeout(view_timeout_event) => 
                self.view_timeout_handlers.iter().for_each(|handler| handler(&view_timeout_event)),

            Event::StartSync(start_sync_event) => 
                self.start_sync_handlers.iter().for_each(|handler| handler(&start_sync_event)),

            Event::EndSync(end_sync_event) => 
                self.end_sync_handlers.iter().for_each(|handler| handler(&end_sync_event)),
        }
    }
}


pub(crate) fn start_event_bus(
    event_handlers: EventHandlers,
    event_subscriber: Receiver<Event>,
    shutdown_signal: Receiver<()>, 
) -> JoinHandle<()> {
    thread::spawn(move || loop {
        match shutdown_signal.try_recv() {
            Ok(()) => return,
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                panic!("event_bus thread disconnected from main thread")
            }
        }

        if let Ok(event) = event_subscriber.try_recv() {
            (&event_handlers).fire_handlers(event)
        } else if let Err(TryRecvError::Disconnected) = event_subscriber.try_recv()  {
            panic!("The algorithm thread (event publisher) was disconnected from the channel")
        }

    })
}