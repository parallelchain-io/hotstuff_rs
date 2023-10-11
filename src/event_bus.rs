//! Event bus thread for handling events published from the algorithm and sync_server threads
//! 
//! When the thread receives a message containing an event, it fires all handlers for the event
//! stored in EventHandlers
//! 
//! When no handlers are stored in a replica's instance of EventHandlers then this thread is not started
//! 
//! A replica's instance of EventHandlers contains the handlers provided upon building the replica,
//! and if logging is enabled via replica's config its instance of EventHandlers also contains
//! the default logging handlers defined in logging.rs

use crate::events::*;
use crate::logging;
use crate::logging::Logger;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::thread::JoinHandle;

pub(crate) type HandlerPtr<T> = Box<dyn Fn(&T) + Send>;

pub(crate) struct EventHandlers {
    pub(crate) insert_block_handlers: Vec<HandlerPtr<InsertBlockEvent>>,
    pub(crate) commit_block_handlers: Vec<HandlerPtr<CommitBlockEvent>>,
    pub(crate) prune_block_handlers: Vec<HandlerPtr<PruneBlockEvent>>,
    pub(crate) update_highest_qc_handlers: Vec<HandlerPtr<UpdateHighestQCEvent>>,
    pub(crate) update_locked_view_handlers: Vec<HandlerPtr<UpdateLockedViewEvent>>,
    pub(crate) update_validator_set_handlers: Vec<HandlerPtr<UpdateValidatorSetEvent>>,

    pub(crate) propose_handlers: Vec<HandlerPtr<ProposeEvent>>,
    pub(crate) nudge_handlers: Vec<HandlerPtr<NudgeEvent>>,
    pub(crate) vote_handlers: Vec<HandlerPtr<VoteEvent>>,
    pub(crate) new_view_handlers: Vec<HandlerPtr<NewViewEvent>>,

    pub(crate) receive_proposal_handlers: Vec<HandlerPtr<ReceiveProposalEvent>>,
    pub(crate) receive_nudge_handlers: Vec<HandlerPtr<ReceiveNudgeEvent>>,
    pub(crate) receive_vote_handlers: Vec<HandlerPtr<ReceiveVoteEvent>>,
    pub(crate) receive_new_view_handlers: Vec<HandlerPtr<ReceiveNewViewEvent>>,

    pub(crate) start_view_handlers: Vec<HandlerPtr<StartViewEvent>>,
    pub(crate) view_timeout_handlers: Vec<HandlerPtr<ViewTimeoutEvent>>,
    pub(crate) collect_qc_handlers: Vec<HandlerPtr<CollectQCEvent>>,

    pub(crate) start_sync_handlers: Vec<HandlerPtr<StartSyncEvent>>,
    pub(crate) end_sync_handlers: Vec<HandlerPtr<EndSyncEvent>>,
    pub(crate) receive_sync_request_handlers: Vec<HandlerPtr<ReceiveSyncRequestEvent>>,
    pub(crate) send_sync_response_handlers: Vec<HandlerPtr<SendSyncResponseEvent>>,
}

impl EventHandlers {

    pub(crate) fn is_empty(&self) -> bool {
        self.insert_block_handlers.is_empty()
        && self.commit_block_handlers.is_empty()
        && self.prune_block_handlers.is_empty()
        && self.update_highest_qc_handlers.is_empty()
        && self.update_locked_view_handlers.is_empty()
        && self.update_validator_set_handlers.is_empty()
        && self.propose_handlers.is_empty()
        && self.nudge_handlers.is_empty()
        && self.vote_handlers.is_empty()
        && self.new_view_handlers.is_empty()
        && self.receive_proposal_handlers.is_empty()
        && self.receive_nudge_handlers.is_empty()
        && self.receive_vote_handlers.is_empty()
        && self.receive_new_view_handlers.is_empty()
        && self.start_view_handlers.is_empty()
        && self.view_timeout_handlers.is_empty()
        && self.collect_qc_handlers.is_empty()
        && self.start_sync_handlers.is_empty()
        && self.end_sync_handlers.is_empty()
        && self.receive_sync_request_handlers.is_empty()
        && self.send_sync_response_handlers.is_empty()
    }

    pub(crate) fn add_logging_handlers(&mut self) {

        self.insert_block_handlers.push(logging::InsertBlockEvent::get_logger());
        self.commit_block_handlers.push(logging::CommitBlockEvent::get_logger());
        self.prune_block_handlers.push(logging::PruneBlockEvent::get_logger());
        self.update_highest_qc_handlers.push(logging::UpdateHighestQCEvent::get_logger());
        self.update_locked_view_handlers.push(logging::UpdateLockedViewEvent::get_logger());
        self.update_validator_set_handlers.push(logging::UpdateValidatorSetEvent::get_logger());

        self.propose_handlers.push(logging::ProposeEvent::get_logger());
        self.nudge_handlers.push(logging::NudgeEvent::get_logger());
        self.vote_handlers.push(logging::VoteEvent::get_logger());
        self.new_view_handlers.push(logging::NewViewEvent::get_logger());

        self.receive_proposal_handlers.push(logging::ReceiveProposalEvent::get_logger());
        self.receive_nudge_handlers.push(logging::ReceiveNudgeEvent::get_logger());
        self.receive_vote_handlers.push(logging::ReceiveVoteEvent::get_logger());
        self.receive_new_view_handlers.push(logging::ReceiveNewViewEvent::get_logger());

        self.start_view_handlers.push(logging::StartViewEvent::get_logger());
        self.view_timeout_handlers.push(logging::ViewTimeoutEvent::get_logger());
        self.collect_qc_handlers.push(logging::CollectQCEvent::get_logger());

        self.start_sync_handlers.push(logging::StartSyncEvent::get_logger());
        self.end_sync_handlers.push(logging::EndSyncEvent::get_logger());
        self.receive_sync_request_handlers.push(logging::ReceiveSyncRequestEvent::get_logger());
        self.send_sync_response_handlers.push(logging::SendSyncResponseEvent::get_logger());

    }

    pub(crate) fn fire_handlers(&self, event: Event) {

        match event {
            Event::InsertBlock(insert_block_event) => 
                self.insert_block_handlers.iter().for_each(|handler| handler(&insert_block_event)),

            Event::CommitBlock(commit_block_event) => 
                self.commit_block_handlers.iter().for_each(|handler| handler(&commit_block_event)),

            Event::PruneBlock(prune_block_event) => 
                self.prune_block_handlers.iter().for_each(|handler| handler(&prune_block_event)),

            Event::UpdateHighestQC(update_highest_qc_event) =>
                self.update_highest_qc_handlers.iter().for_each(|handler| handler(&update_highest_qc_event)),

            Event::UpdateLockedView(update_locked_view_event) =>
                self.update_locked_view_handlers.iter().for_each(|handler| handler(&update_locked_view_event)),

            Event::UpdateValidatorSet(update_validator_set_event) =>
                self.update_validator_set_handlers.iter().for_each(|handler| handler(&update_validator_set_event)),

            Event::Propose(propose_event) => 
                self.propose_handlers.iter().for_each(|handler| handler(&propose_event)),

            Event::Nudge(nudge_event) => 
                self.nudge_handlers.iter().for_each(|handler| handler(&nudge_event)),

            Event::Vote(vote_event) => 
                self.vote_handlers.iter().for_each(|handler| handler(&vote_event)),

            Event::NewView(new_view_event) => 
                self.new_view_handlers.iter().for_each(|handler| handler(&new_view_event)),

            Event::ReceiveProposal(receive_proposal_event) => 
                self.receive_proposal_handlers.iter().for_each(|handler| handler(&receive_proposal_event)),

            Event::ReceiveNudge(receive_nudge_event) => 
                self.receive_nudge_handlers.iter().for_each(|handler| handler(&receive_nudge_event)),

            Event::ReceiveVote(receive_vote_event) => 
                self.receive_vote_handlers.iter().for_each(|handler| handler(&receive_vote_event)),

            Event::ReceiveNewView(receive_new_view) => 
                self.receive_new_view_handlers.iter().for_each(|handler| handler(&receive_new_view)),

            Event::StartView(start_view_event) => 
                self.start_view_handlers.iter().for_each(|handler| handler(&start_view_event)),

            Event::ViewTimeout(view_timeout_event) => 
                self.view_timeout_handlers.iter().for_each(|handler| handler(&view_timeout_event)),

            Event::CollectQC(collect_qc_event) =>
                self.collect_qc_handlers.iter().for_each(|handler| handler(&collect_qc_event)),

            Event::StartSync(start_sync_event) => 
                self.start_sync_handlers.iter().for_each(|handler| handler(&start_sync_event)),

            Event::EndSync(end_sync_event) => 
                self.end_sync_handlers.iter().for_each(|handler| handler(&end_sync_event)),

            Event::ReceiveSyncRequest(receive_sync_request_event) =>
                self.receive_sync_request_handlers.iter().for_each(|handler| handler(&receive_sync_request_event)),

            Event::SendSyncResponse(send_sync_response_event) =>
                self.send_sync_response_handlers.iter().for_each(|handler| handler(&send_sync_response_event)),
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
            panic!("The algorithm thread (event publisher) disconnected from the channel")
        }

    })
}