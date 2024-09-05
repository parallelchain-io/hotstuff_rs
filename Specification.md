# HotStuff-rs specification

## Block Tree

### Fields

#### Locked QC

TODO Wednesday.

#### Highest QC

TODO Wednesday.

#### Validator Sets State

### Mutators

#### Insert

TODO Wednesday.

#### Update

TODO Wednesday.

### Safety predicates

#### Safe QC

TODO Tuesday.

#### Safe Block

TODO Tuesday.

#### Safe Nudge

TODO Tuesday.

#### Repropose Block

TODO Wednesday.

### Update predicates

#### QC to Lock 

TODO Tuesday.

#### Block to Commit

TODO Tuesday.

## HotStuff

### Types

#### Quorum Certificate

TODO Wednesday.

### State

#### Vote Collector

TODO Tuesday.

#### Keypair

#### App

#### Current View

### Actions

#### Enter View

Code: `hotstuff_rs::hotstuff::protocol::HotStuff::enter_view`

```rust
fn enter_view(view: ViewNum) {
    // 1. Create a NewView message for the current view and send it to the next leader(s).
    let new_view = NewView {
        chain_id,
        view: current_view,
        highest_qc: block_tree.highest_qc(),
    }

    for leader in new_view_recipients(&new_view, block_tree.validator_sets_state()) {
        network.send(leader, new_view);
    }

    // 2. Update the HotStuff subprotocol's copy of the current view.
    current_view = view;

    // 3. Replace the existing vote collectors with new ones for the current view.
    vote_collectors = VoteCollector::new(chain_id, current_view, block_tree.validator_sets_state());

    // 4. If I am a proposer for the newly-entered view, then broadcast a `Proposal` or a `Nudge`.
    if is_proposer(
        keypair.verifying(),
        view,
        block_tree.validator_sets_state(),
    ) {
        // 4.1. If a chain of consecutive views justifying a validator set updating block has been broken,
        // re-propose the validator set updating block.
        if let Some(block) = block_tree.repropose_block(view) {
            let proposal = Proposal {
                chain_id,
                view,
                block,
            }

            network.broadcast(proposal);
        }

        // 4.2. Otherwise, decide whether to broadcast a new proposal, or a new nudge, according to phase of the highest QC.
        else {
            match block_tree.highest_qc().phase {

                // 4.2.1. If the phase of the highest QC is Generic or Decide, create a new Proposal and broadcast it.
                Phase::Generic | Phase::Decide => {
                    let block = app.produce_block(&block_tree, block_tree.highest_qc());
                    let proposal = Proposal {
                        chain_id,
                        view,
                        block,
                    }

                    network.broadcast(proposal);
                },

                // 4.2.2. If the phase of the highest QC is Prepare, Precommit, or Commit, create a new Nudge and broadcast it.
                Phase::Prepare | Phase::Precommit | Phase::Commit => {
                    let nudge = Nudge {
                        chain_id,
                        view,
                        justify: block_tree.highest_qc(),
                    }

                    network.broadcast(nudge);
                }
            }
        }
    }
}
```

#### On Receive Proposal

Code: `hotstuff_rs::hotstuff::protocol::HotStuff::on_receive_proposal`

```rust
fn on_receive_proposal(proposal: Proposal, origin: VerifyingKey) {
    // 1. Confirm that `origin` really is a proposer in the current view. 
    if is_proposer(origin, current_view, block_tree.validator_set_state()) {

        // 2. Confirm that `proposal.block` is safe according to the rules of the block tree.
        if block_tree.safe_block(&proposal.block, chain_id) {

            // 3. Confirm that `proposal.block` is valid according to the rules of the app.
            if let Ok((app_state_updates, validator_set_updates)) = app.validate_block(&block_tree) {

                // 4. Insert `proposal.block` into the block tree.
                block_tree.insert(proposal.block, app_state_updates, validator_set_updates);

                // 5. Update the block tree using `proposal.block.justify`.
                block_tree.update(&proposal.block.justify);

                // 6. Tell the vote collectors to start collecting votes according to the new validator sets state (which
                // may or may not have been changed in the block tree update in the previous step).
                vote_collectors.update_validator_sets(block_tree.validator_sets_state());

                // 7. If the local replica's votes can become part of QCs that directly extend `proposal.block.justify`,
                //    vote for `proposal`.
                if is_voter(
                    keypair.public(),
                    block_tree.validator_sets_state(),
                    &proposal.block.justify,
                ) {
                    // 7.1. Compute the phase to vote in: if `proposal.block` updates the validator set, then vote in the
                    //      `Prepare` phase. Otherwise, vote in the `Generic` phase.
                    let vote_phase = if validator_set_updates.is_some() {
                        Phase::Prepare
                    } else {
                        Phase::Generic
                    }
                    let vote = Vote::new(
                        keypair,
                        chain_id,
                        current_view,
                        proposal.block.hash,
                        vote_phase,
                    );

                    // 7.2. Send the vote to the leader that should receive it.
                    network.send(vote, vote_recipient(&vote, block_tree.validator_sets_state()));
                }
            }
        }
    }
}
```

#### On Receive Nudge

```rust
fn on_receive_nudge(nudge: Nudge, origin: VerifyingKey) {
    // 1. Confirm that `origin` really is a proposer in the current view. 
    if is_proposer(origin, current_view, block_tree.validator_set_state()) {

        // 2. Confirm that `nudge` is safe according to the rules of the block tree.
        if block_tree.safe_nudge(&nudge, current_view, chain_id) {

            // 3. Update the block tree using `nudge.justify`.
            block_tree.update(&nudge.justify);

            // 4. Tell the vote collectors to start collecting votes according to the new validator sets state (which
            // may or may not have been changed in the block tree update in the previous step).
            vote_collectors.update_validator_sets(block_tree.validator_sets_state());

            // 5. If the local replica's votes can become part of QCs that directly extend `nudge.justify`, vote for
            //    `nudge`.
            if is_voter(
                keypair.public(),
                block_tree.validator_sets_state(),
                &nudge.justify,
            ) {
                // 5.1. Compute the phase to vote in: this will be the phase that follows `nudge.justify.phase`.
                let vote_phase = match nudge.justify.phase {
                    Phase::Prepare => Phase::Precommit,
                    Phase::Precommit => Phase::Commit,
                    Phase::Commit => Phase::Decide,
                    _ => unreachable!("`safe_nudge` should have ensured that `nudge.justify.phase` is neither `Generic` or `Decide`"),
                };
                let vote = Vote::new(
                    keypair,
                    chain_id,
                    current_view,
                    proposal
                )

                // 5.2. Send the vote to the leader that should receive it.
                network.send(vote, vote_recipient(&vote, block_tree.validator_sets_state()))
            }
        }
    }
}
```

#### On Receive Vote

```rust
fn on_receive_vote(vote: Vote, origin: VerifyingKey) {
    // 1. Confirm that `vote` was signed by `origin`.
    if vote.is_correct(origin) {

        // 2. Collect `vote` using the vote collectors.
        let new_qc = vote_collectors.collect(vote, origin);

        // 3. If sufficient votes were collected to form a `new_qc`, use `new_qc` to update the block tree. 
        if let Some(new_qc) = new_qc {
            // 3.1. Confirm that `new_qc` is safe according to the rules of the block tree. 
            // 
            // Note (TODO): I can think of at least three ways this check can fail:
            // 1. A quorum of replicas are byzantine and form a QC with an illegal phase, that is:
            //     1. A Generic QC that justifies a VSU-block.
            //     2. A non-Generic QC that justifies a non-VSU-block.
            // 2. We forgot to create a new vote collector with a higher view in `enter_view` (library bug). 
            // 3. We collected a QC for a block that isn't in the block tree yet (block sync may help).
            if block_tree.safe_qc(new_qc) {

                // 3.2. Update the block tree using `new_qc`.
                block_tree.update(new_qc);

                // 3.3. Tell the vote collectors to start collecting votes according to the new validator sets state (which
                // may or may not have been changed in the block tree update in the previous step). 
                vote_collectors.update_validator_sets(block_tree.validator_set_state());
            }
        }
    }
}
```

#### On Receive New View

```rust
fn on_receive_new_view(new_view: NewView, origin: VerifyingKey) {
    // 1. Confirm that `new_qc` is safe according to the rules of the block tree. 
    if block_tree.safe_qc(&new_view.highest_qc) {

        // 2. Update the block tree using `new_view.highest_qc`.
        block_tree.update(new_view.highest_qc);

        // 3. Tell the vote collectors to start collecting votes according to the new validator sets state (which
        // may or may not have been changed in the block tree update in the previous step). 
        vote_collectors.update_validator_sets(block_tree.validator_set_state());
    }
}
```

### Role predicates

#### Is Proposer

Code and Specification: `hotstuff_rs::hotstuff::roles::is_proposer`

#### Is Voter

Code and Specification: `hotstuff_rs::hotstuff::roles::is_voter`

#### Vote Recipient

Code and Specification: `hotstuff_rs::hotstuff::roles::vote_recipient`

#### New View Recipients

Code and Specification: `hotstuff_rs::hotstuff::roles::new_view_recipients`
