/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Thread that receives events emitted by the [algorithm](crate::algorithm) and
//! [block sync server](crate::block_sync::server::BlockSyncServer) threads and passes them to event
//! handlers.
//!
//! When the thread receives a message containing an [event](crate::events::Event), it triggers the
//! execution of all handlers defined for the contained event type, where the handlers for each event
//! type are stored in [`EventHandlers`].
//!
//! When no handlers are present in a replica's instance of `EventHandlers` this thread is not started.
//!
//! ## Event Handlers
//!
//! A replica's instance of `EventHandlers` contains:
//! 1. The handlers provided upon building the replica via [`ReplicaSpec`](crate::replica::ReplicaSpec),
//!    and
//! 2. If logging is enabled via replica's [config](crate::replica::Configuration) then also
//!    the default logging handlers defined in [logging](crate::logging).

use std::{
    sync::mpsc::{Receiver, TryRecvError},
    thread,
    thread::JoinHandle,
};

use crate::{events::*, logging::Logger};

/// Pointer to a handler closure, parametrised by the argument (for our use case, event) type.
pub(crate) type HandlerPtr<T> = Box<dyn Fn(&T) + Send>;

/// Stores the two optional handlers enabled for an event type that implements the [`Logger`] trait,
/// namely one logging handler, defined in [`logging`](crate::logging), and one user-defined handler,
/// passed to [`ReplicaSpec`](crate::replica::ReplicaSpec).
///
/// Note that the user-defined handler is expected to include all expected event-handling
/// functionalities per event.
pub(crate) struct HandlerPair<T: Logger> {
    pub(crate) user_defined_handler: Option<HandlerPtr<T>>,
    pub(crate) logging_handler: Option<HandlerPtr<T>>,
}

impl<T: Logger> HandlerPair<T> {
    // Checks if no event handlers are defined for this event.
    pub(crate) fn is_empty(&self) -> bool {
        self.user_defined_handler.is_none() && self.logging_handler.is_none()
    }

    /// Creates a new `HandlerPair` with the user-defined handler, and the default logging
    /// handler if logging is enabled.
    pub(crate) fn new(log: bool, user_defined_handler: Option<HandlerPtr<T>>) -> HandlerPair<T> {
        HandlerPair {
            user_defined_handler,
            logging_handler: if log { Some(T::get_logger()) } else { None },
        }
    }
}

/// Stores the `HandlerPair` of user-defined and optional logging handlers for each pre-defined event
/// type from [events](crate::events).
pub(crate) struct EventHandlers {
    pub(crate) insert_block_handlers: HandlerPair<InsertBlockEvent>,
    pub(crate) commit_block_handlers: HandlerPair<CommitBlockEvent>,
    pub(crate) prune_block_handlers: HandlerPair<PruneBlockEvent>,
    pub(crate) update_highest_pc_handlers: HandlerPair<UpdateHighestPCEvent>,
    pub(crate) update_locked_pc_handlers: HandlerPair<UpdateLockedPCEvent>,
    pub(crate) update_highest_tc_handlers: HandlerPair<UpdateHighestTCEvent>,
    pub(crate) update_validator_set_handlers: HandlerPair<UpdateValidatorSetEvent>,

    pub(crate) propose_handlers: HandlerPair<ProposeEvent>,
    pub(crate) nudge_handlers: HandlerPair<NudgeEvent>,
    pub(crate) phase_vote_handlers: HandlerPair<PhaseVoteEvent>,
    pub(crate) new_view_handlers: HandlerPair<NewViewEvent>,
    pub(crate) timeout_vote_handlers: HandlerPair<TimeoutVoteEvent>,
    pub(crate) advance_view_handlers: HandlerPair<AdvanceViewEvent>,

    pub(crate) receive_proposal_handlers: HandlerPair<ReceiveProposalEvent>,
    pub(crate) receive_nudge_handlers: HandlerPair<ReceiveNudgeEvent>,
    pub(crate) receive_phase_vote_handlers: HandlerPair<ReceivePhaseVoteEvent>,
    pub(crate) receive_new_view_handlers: HandlerPair<ReceiveNewViewEvent>,
    pub(crate) receive_timeout_vote_handlers: HandlerPair<ReceiveTimeoutVoteEvent>,
    pub(crate) receive_advance_view_handlers: HandlerPair<ReceiveAdvanceViewEvent>,

    pub(crate) start_view_handlers: HandlerPair<StartViewEvent>,
    pub(crate) view_timeout_handlers: HandlerPair<ViewTimeoutEvent>,
    pub(crate) collect_pc_handlers: HandlerPair<CollectPCEvent>,
    pub(crate) collect_tc_handlers: HandlerPair<CollectTCEvent>,

    pub(crate) start_sync_handlers: HandlerPair<StartSyncEvent>,
    pub(crate) end_sync_handlers: HandlerPair<EndSyncEvent>,
    pub(crate) receive_sync_request_handlers: HandlerPair<ReceiveSyncRequestEvent>,
    pub(crate) send_sync_response_handlers: HandlerPair<SendSyncResponseEvent>,
}

impl EventHandlers {
    /// Creates the [handler pairs](HandlerPair) for all pre-defined event types from
    /// [events](crate::events) given the user-defined handlers, and information on whether logging is
    /// enabled.
    pub(crate) fn new(
        log: bool,
        insert_block_handler: Option<HandlerPtr<InsertBlockEvent>>,
        commit_block_handler: Option<HandlerPtr<CommitBlockEvent>>,
        prune_block_handler: Option<HandlerPtr<PruneBlockEvent>>,
        update_highest_pc_handler: Option<HandlerPtr<UpdateHighestPCEvent>>,
        update_locked_pc_handler: Option<HandlerPtr<UpdateLockedPCEvent>>,
        update_highest_tc_handler: Option<HandlerPtr<UpdateHighestTCEvent>>,
        update_validator_set_handler: Option<HandlerPtr<UpdateValidatorSetEvent>>,
        propose_handler: Option<HandlerPtr<ProposeEvent>>,
        nudge_handler: Option<HandlerPtr<NudgeEvent>>,
        phase_vote_handler: Option<HandlerPtr<PhaseVoteEvent>>,
        new_view_handler: Option<HandlerPtr<NewViewEvent>>,
        timeout_vote_handler: Option<HandlerPtr<TimeoutVoteEvent>>,
        advance_view_handler: Option<HandlerPtr<AdvanceViewEvent>>,
        receive_proposal_handler: Option<HandlerPtr<ReceiveProposalEvent>>,
        receive_nudge_handler: Option<HandlerPtr<ReceiveNudgeEvent>>,
        receive_phase_vote_handler: Option<HandlerPtr<ReceivePhaseVoteEvent>>,
        receive_new_view_handler: Option<HandlerPtr<ReceiveNewViewEvent>>,
        receive_timeout_vote_handler: Option<HandlerPtr<ReceiveTimeoutVoteEvent>>,
        receive_advance_view_handler: Option<HandlerPtr<ReceiveAdvanceViewEvent>>,
        start_view_handler: Option<HandlerPtr<StartViewEvent>>,
        view_timeout_handler: Option<HandlerPtr<ViewTimeoutEvent>>,
        collect_pc_handler: Option<HandlerPtr<CollectPCEvent>>,
        collect_tc_handler: Option<HandlerPtr<CollectTCEvent>>,
        start_sync_handler: Option<HandlerPtr<StartSyncEvent>>,
        end_sync_handler: Option<HandlerPtr<EndSyncEvent>>,
        receive_sync_request_handler: Option<HandlerPtr<ReceiveSyncRequestEvent>>,
        send_sync_response_handler: Option<HandlerPtr<SendSyncResponseEvent>>,
    ) -> EventHandlers {
        EventHandlers {
            insert_block_handlers: HandlerPair::new(log, insert_block_handler),
            commit_block_handlers: HandlerPair::new(log, commit_block_handler),
            prune_block_handlers: HandlerPair::new(log, prune_block_handler),
            update_highest_pc_handlers: HandlerPair::new(log, update_highest_pc_handler),
            update_locked_pc_handlers: HandlerPair::new(log, update_locked_pc_handler),
            update_highest_tc_handlers: HandlerPair::new(log, update_highest_tc_handler),
            update_validator_set_handlers: HandlerPair::new(log, update_validator_set_handler),
            propose_handlers: HandlerPair::new(log, propose_handler),
            nudge_handlers: HandlerPair::new(log, nudge_handler),
            phase_vote_handlers: HandlerPair::new(log, phase_vote_handler),
            new_view_handlers: HandlerPair::new(log, new_view_handler),
            timeout_vote_handlers: HandlerPair::new(log, timeout_vote_handler),
            advance_view_handlers: HandlerPair::new(log, advance_view_handler),
            receive_proposal_handlers: HandlerPair::new(log, receive_proposal_handler),
            receive_nudge_handlers: HandlerPair::new(log, receive_nudge_handler),
            receive_phase_vote_handlers: HandlerPair::new(log, receive_phase_vote_handler),
            receive_new_view_handlers: HandlerPair::new(log, receive_new_view_handler),
            receive_timeout_vote_handlers: HandlerPair::new(log, receive_timeout_vote_handler),
            receive_advance_view_handlers: HandlerPair::new(log, receive_advance_view_handler),
            start_view_handlers: HandlerPair::new(log, start_view_handler),
            view_timeout_handlers: HandlerPair::new(log, view_timeout_handler),
            collect_pc_handlers: HandlerPair::new(log, collect_pc_handler),
            collect_tc_handlers: HandlerPair::new(log, collect_tc_handler),
            start_sync_handlers: HandlerPair::new(log, start_sync_handler),
            end_sync_handlers: HandlerPair::new(log, end_sync_handler),
            receive_sync_request_handlers: HandlerPair::new(log, receive_sync_request_handler),
            send_sync_response_handlers: HandlerPair::new(log, send_sync_response_handler),
        }
    }

    /// Checks if no handlers are defined, i.e., neither user-defined handlers were defined nor logging is
    /// enabled.
    pub(crate) fn is_empty(&self) -> bool {
        self.insert_block_handlers.is_empty()
            && self.commit_block_handlers.is_empty()
            && self.prune_block_handlers.is_empty()
            && self.update_highest_pc_handlers.is_empty()
            && self.update_locked_pc_handlers.is_empty()
            && self.update_highest_tc_handlers.is_empty()
            && self.update_validator_set_handlers.is_empty()
            && self.propose_handlers.is_empty()
            && self.nudge_handlers.is_empty()
            && self.phase_vote_handlers.is_empty()
            && self.new_view_handlers.is_empty()
            && self.timeout_vote_handlers.is_empty()
            && self.advance_view_handlers.is_empty()
            && self.receive_proposal_handlers.is_empty()
            && self.receive_nudge_handlers.is_empty()
            && self.receive_phase_vote_handlers.is_empty()
            && self.receive_new_view_handlers.is_empty()
            && self.receive_timeout_vote_handlers.is_empty()
            && self.receive_advance_view_handlers.is_empty()
            && self.start_view_handlers.is_empty()
            && self.view_timeout_handlers.is_empty()
            && self.collect_pc_handlers.is_empty()
            && self.collect_tc_handlers.is_empty()
            && self.start_sync_handlers.is_empty()
            && self.end_sync_handlers.is_empty()
            && self.receive_sync_request_handlers.is_empty()
            && self.send_sync_response_handlers.is_empty()
    }

    /// Triggers the execution of each of the two handlers - the user-defined and the logging handler, if
    /// defined - for a given event type from [events](crate::events).
    pub(crate) fn fire_handlers(&self, event: Event) {
        match event {
            Event::InsertBlock(insert_block_event) => {
                self.insert_block_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&insert_block_event));
                self.insert_block_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&insert_block_event));
            }
            Event::CommitBlock(commit_block_event) => {
                self.commit_block_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&commit_block_event));
                self.commit_block_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&commit_block_event));
            }
            Event::PruneBlock(prune_block_event) => {
                self.prune_block_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&prune_block_event));
                self.prune_block_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&prune_block_event));
            }
            Event::UpdateHighestPC(update_highest_pc_event) => {
                self.update_highest_pc_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&update_highest_pc_event));
                self.update_highest_pc_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&update_highest_pc_event));
            }
            Event::UpdateLockedPC(update_locked_pc_event) => {
                self.update_locked_pc_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&update_locked_pc_event));
                self.update_locked_pc_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&update_locked_pc_event));
            }
            Event::UpdateHighestTC(update_highest_tc_event) => {
                self.update_highest_tc_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&update_highest_tc_event));
                self.update_highest_tc_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&update_highest_tc_event));
            }
            Event::UpdateValidatorSet(update_validator_set_event) => {
                self.update_validator_set_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&update_validator_set_event));
                self.update_validator_set_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&update_validator_set_event));
            }
            Event::Propose(propose_event) => {
                self.propose_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&propose_event));
                self.propose_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&propose_event));
            }
            Event::Nudge(nudge_event) => {
                self.nudge_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&nudge_event));
                self.nudge_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&nudge_event));
            }
            Event::PhaseVote(phase_vote_event) => {
                self.phase_vote_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&phase_vote_event));
                self.phase_vote_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&phase_vote_event));
            }
            Event::NewView(new_view_event) => {
                self.new_view_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&new_view_event));
                self.new_view_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&new_view_event));
            }
            Event::TimeoutVote(timeout_vote_event) => {
                self.timeout_vote_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&timeout_vote_event));
                self.timeout_vote_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&timeout_vote_event));
            }
            Event::AdvanceView(advance_view_event) => {
                self.advance_view_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&advance_view_event));
                self.advance_view_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&advance_view_event));
            }
            Event::ReceiveProposal(receive_proposal_event) => {
                self.receive_proposal_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&receive_proposal_event));
                self.receive_proposal_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&receive_proposal_event));
            }
            Event::ReceiveNudge(receive_nudge_event) => {
                self.receive_nudge_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&receive_nudge_event));
                self.receive_nudge_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&receive_nudge_event));
            }
            Event::ReceivePhaseVote(receive_vote_event) => {
                self.receive_phase_vote_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&receive_vote_event));
                self.receive_phase_vote_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&receive_vote_event));
            }
            Event::ReceiveNewView(receive_new_view) => {
                self.receive_new_view_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&receive_new_view));
                self.receive_new_view_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&receive_new_view));
            }
            Event::ReceiveTimeoutVote(receive_timeout_vote_event) => {
                self.receive_timeout_vote_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&receive_timeout_vote_event));
                self.receive_timeout_vote_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&receive_timeout_vote_event));
            }
            Event::ReceiveAdvanceView(receive_advance_view_event) => {
                self.receive_advance_view_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&receive_advance_view_event));
                self.receive_advance_view_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&receive_advance_view_event));
            }
            Event::StartView(start_view_event) => {
                self.start_view_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&start_view_event));
                self.start_view_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&start_view_event));
            }
            Event::ViewTimeout(view_timeout_event) => {
                self.view_timeout_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&view_timeout_event));
                self.view_timeout_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&view_timeout_event));
            }
            Event::CollectPC(collect_pc_event) => {
                self.collect_pc_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&collect_pc_event));
                self.collect_pc_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&collect_pc_event));
            }
            Event::CollectTC(collect_tc_event) => {
                self.collect_tc_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&collect_tc_event));
                self.collect_tc_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&collect_tc_event));
            }
            Event::StartSync(start_sync_event) => {
                self.start_sync_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&start_sync_event));
                self.start_sync_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&start_sync_event));
            }
            Event::EndSync(end_sync_event) => {
                self.end_sync_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&end_sync_event));
                self.end_sync_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&end_sync_event));
            }
            Event::ReceiveSyncRequest(receive_sync_request_event) => {
                self.receive_sync_request_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&receive_sync_request_event));
                self.receive_sync_request_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&receive_sync_request_event));
            }
            Event::SendSyncResponse(send_sync_response_event) => {
                self.send_sync_response_handlers
                    .user_defined_handler
                    .iter()
                    .for_each(|handler| handler(&send_sync_response_event));
                self.send_sync_response_handlers
                    .logging_handler
                    .iter()
                    .for_each(|handler| handler(&send_sync_response_event));
            }
        }
    }
}

/// Starts the event bus thread, which runs an infinite loop until a shutdown signal is received from
/// the parent thread. In each iteration of the loop, the thread checks if it received any event
/// notifications, and if so, then triggers the execution of the handlers defined for the event.
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
        } else if let Err(TryRecvError::Disconnected) = event_subscriber.try_recv() {
            panic!("The algorithm thread (event publisher) disconnected from the channel")
        }
    })
}
