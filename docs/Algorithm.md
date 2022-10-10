# HotStuff-rs Protocol State Machine

The *Protocol State Machine* works to extend the BlockTree. Starting with *Sync*, the flow cycles between 6 states: *Sync*, *BeginView*, *Leader*, *Replica*, *NextLeader*, and *NewView*. This section documents Progress Mode behavior in terms of its steps. To aid with understanding, we group the steps of the Leader and Replica states in terms of high-level 'phases'.

## State Machine 

![A UML-esque State Machine Diagram depicting the states the Engine Thread can be in and the transition between states.](../readme_assets/Engine%20State%20Machine%20Diagram.png)

### Sync

Before Consensus can make Progress, a quorum of Participants have to be synchronized on: 1. The same View Number, and 2. The same `generic_qc`*.

The flow proceeds in the following sequence of steps:
1. Query all Participants for a chain of length max `CATCHUP_API_WINDOW_SIZE` extending from the latest committed Block of local BlockTree. 
2. Wait `CATCHUP_API_TIMEOUT` milliseconds.
3. Select the longest `chain` from all responses.
4. If `len(chain) == CATCHUP_API_WINDOW_SIZE`, execute and insert the chain into the local Block Tree, then jump back to step 1.
    1. Else if `len(chain) < CATCHUP_API_WINDOW_SIZE`, execute and insert the chain into the local Block Tree.
    2. If a Block fails validation in the 'execute and insert' step, panic (this indicates that more than $f$ Participants are faulty at this moment).
5. Transition to *BeginView*. 

**BeginView**

1. `cur_view = max(cur_view, generic_qc.view_number + 1)`
2. If `leader(cur_view) == me`: transition to *Leader*, else, transition to *Replica*.

**Leader**

<u>Phase 1: Produce a new Block</u>

1. Call `block = create_leaf()` on Application with `parent = generic_qc.block`.
2. Insert `block` into local BlockTree.

<u>Phase 2: Broadcast a Proposal.</u>

3. Broadcast a new `PROPOSE` message containing the leaf to every participant.
4. Send a `VOTE` for our own proposal to the next leader.
5. If I am the next leader: transition to *NextLeader* with `timeout` = `TNT` - `time since Phase 1 began`, else continue.

<u>Phase 3: Wait for Replicas to send vote for proposal to the next leader.</u>

6. Sleep for `2 * EXPECTED_WORST_CASE_NET_LATENCY + time elapsed since Phase 1 began` seconds or until the the view timeout, whichever is sooner.
7. Set `cur_view += 1` and transition to *BeginView*.

**Replica** 

<u>Phase 1: Wait for a Proposal.</u>

1. Read messages from the current leader **until** a matching proposal is received:
    1. If message is a `PROPOSE`:
        1. If `propose.view_number < cur_view`: discard it.
        2. Else if `propose.block.justify.block_hash` is not in the local BlockTree: transition to *Sync*.
        3. Else if `propose.view_number > cur_view`: discard it.
        4. Else (if `propose.view_number == cur_view`): evaluate the **SafeBlock** predicate on it. If it evaluates to true, break. Else, discard it.
    2. If message is a `VOTE`: discard it.
    3. If message is a `NEW-VIEW`:
        1. If `new_view.view_number < cur_view - 1`: discard it.
        2. Else if `new_view.qc.view_number > generic_qc.view_number`: transition to *Sync*.
        3. Else: discard it.

<u>Phase 2: Validate the proposed Block.</u>

2. Call `execute` on App with `block = proposal.block`.
    - If Application rejects the block, transition to *NewView*.
3. Insert `block` into the local BlockTree.

<u>Phase 3: Send a vote.</u>

4. Send out a `VOTE` containing `block_hash = proposal.block.hash()`.
5. If I am the next leader: transition to `NextLeader` with `timeout` = `TNT - time elapsed since phase 1 began`, else continue.

<u>Phase 4: Wait for the next leader to finish collecting votes.</u>

6. Sleep for `EXPECTED_WORST_CASE_NET_LATENCY` seconds.
7. Set `cur_view += 1` and transition to *BeginView*.

**NextLeader(`timeout`)**

1. Read messages from *every* participant **until** `timeout` is elapsed  **or** until a new QC is formed:
    1. If message is a `VOTE`:
        1. If `vote.view_number < cur_view`: discard it.
        2. Else if `vote.block_hash` is not in the local BlockTree: transition to *Sync*.
        3. Else if `vote.view_number > cur_view`: discard it.
        4. Else (if `vote.view_number == cur_view`): `if Ok(qc) = qc_builder.insert { generic_qc = qc }`.
    2. If message is a `NEW-VIEW`:
        1. If `new_view.view_number < cur_view - 1`: discard it.
        2. Else if `new_view.qc.view_number > generic_qc.view_number`: transition to *Sync*.
        3. Else: discard it.
    3. If message is a `PROPOSE`: discard it. 
2. Set `cur_view += 1` and transition to *BeginView*.

**NewView**

This state is entered when a *View Timeout* is triggered at any point in the Engine's lifetime.

1. Send out a `NEW-VIEW` containing `view_number` and `generic_qc`.
2. Set `cur_view += 1` and transition to *BeginView*.

## View Timeouts

View timeouts determine how long the Progress Mode State Machine remains on a View before deciding that further progress at the current View is unlikely and moving onto the next View.

If View timeout is a constant and Participants' View Numbers become unsynchronized at any point in the protocol's lifetime, then Participants will never become synchronized at the same View ever again, preventing further progress.

In order to ensure that Participants eventually synchronize on the same View Number for long enough to make progress, the duration of a View timeout grows exponentially in terms of the number of consecutive views the Participant fails to insert a new block:

```rust
Timeout(cur_view, generic_qc) = TNT + 2 ** (cur_view - generic_qc.view_number) seconds
```

## Duration of thread sleeps

![A graphic with 2 rows of 4 rectangles each. The first row depicts the phases of Leader execution, the second depicts the phases of Replica execution.](../readme_assets/Engine%20Timing%20Diagram.png)

## Leader selection

## Progress Mode IPC

Participants try maintain a one-to-one TCP connection with every other Participant. In order 