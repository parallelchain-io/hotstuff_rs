# HotStuff-rs 
HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
1. Guaranteed Safety and Liveness in the face of up to 1/3rd of Voting Power being Byzantine at any given moment,
2. A small API (`Executor`) for plugging in state machine-based applications like blockchains, and
3. Well-documented, 'obviously correct' source code, designed for easy analysis and extension.

## 1. Reading this document
1. If you just want to *use* HotStuff-rs in your application: read Sections 2 and 3.5.
2. If you want to *understand* how HotStuff works: read the HotStuff [paper](./readme_assets/HotStuff%20paper.pdf).
3. If you want to *contribute* to HotStuff-rs' development: read the HotStuff paper, then read this entire document. 

## 2. The HotStuff Consensus Protocol

HotStuff works by building a 'NodeTree': a directed acyclic graph of Nodes. Node is a structure with a `command` field which applications are free to populate with arbitrary byte-arrays. In consensus algorithm literature, we typically talk of consensus algorithms as maintaining state machines that change their internal states in response to commands, hence, the choice of terminology.

HotStuff guarantees that committed Nodes are *immutable*. That is, they can never be *un*-committed as long as at least a supermajority of voting power faithfully execute the protocol. This guarantee enables applications to make hard-to-reverse actions with confidence. 

![A graphic depicting a Tree (DAG) of nodes. Nodes are colored depending on how many confirmations they have.](./readme_assets/NodeTree.png "NodeTree")

A Node becomes *committed* the instant its third confirmation is written into the NodeTree. A confirmation for a Node `A` is another Node `B` such that there is path between `B` to `A`.

The choice of third confirmation to define commitment--as opposed to first or second--is not arbitrary. HotStuff's safety and liveness properties actually hinge upon on this condition. If you really want to understand why this is the case, you should read the [paper](./readme_assets/HotStuff%20paper.pdf). To summarize:

1. Classic BFT consensus algorithms such as PBFT require only 2 confirmations for commitment, but this comes at the cost of expensive leader-replacement flows.
2. Tendermint require only 2 confirmations for commitment and has a simple leader-replacement flow, but needs an explicit 'wait-for-N seconds' step to guarantee liveness.

HotStuff is the first consensus algorithm with a simple leader-replacement algorithm that does not have a 'wait-for-N seconds' step, and thus can make progress as fast as network latency allows.

## 3. Overview of Major Components

![A graphic depicting the set of relationships between HotStuff-rs' components](./readme_assets/Components.png "Components")

### 3.1. Engine Thread

#### 3.1.1. Sync Mode

#### 3.1.2. Progress Mode 

### 3.2. Consensus TCP API

The Consensus TCP API carries `PROPOSE`, `VOTE`, and `NEW-VIEW` messages between Replicas. When all Nodes are up-to-date, this is the only HotStuff-rs network API that will see any traffic. 

### 3.3. NodeTree HTTP API

The NodeTree HTTP API serves requests for Nodes in NodeTree. This is used by out-of-date Replicas to catch up to the head of the NodeTree.  

### 3.4. Connection Manager

### 3.5. Your Application

Anything that *you*, the HotStuff-rs user, can imagine. The Actor thread expects it to implement a trait `Application`.

## 4. Protocol Description

### 4.1. Types 

#### **NODE(command, justify)**
|Field |Type |Description |
|---   |---  |---         |
|command |`Vec<u8>` | |
|justify |`QC` | |

#### **NodeHash**
SHA256Hash over `parent ++ command ++ justify`.

#### **QC(view_num, node_hash, sigs)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node_hash |`NodeHash` | |
|sigs |`Vec<(PublicAddress, Signature)>` | |

#### **Sig(public_address, signature)**
|Field |Type |Description |
|---   |---  |---         |
|public_address |`SHA256Hash` | |
|signature |`Ed25519Signature`

### 4.2. Consensus API Messages

#### **PROPOSE(view_number, node)**
|Field |Type |Description |
|---   |---  |---         |
|view_number |`u64` | |
|node |`Node` | |

#### **VOTE(view_num, node_hash, sig)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node_hash |`NodeHash` | |
|sig |`Sig` | |

#### **NEW-VIEW(view_num, generic_qc)**
|Field |Type |Description |
|---   |---  |---         |
|view_num|`u64` | |
|generic_qc |`QC` | |

### 4.3. NodeTree API Endpoints

### 4.4. Engine Thread Sequence Flow 

### 4.4.1. Initialization

### 4.4.2. Sync Mode

Before Consensus can make Progress, a quorum of Participants have to be synchronized on: 1. The same View Number, and 2. The same `generic_qc`*. 

The flow proceeds in the following sequence of steps:
1. Query all Participants for a chain of length max `CATCHUP_API_WINDOW_SIZE` extending from the latest committed Node of local NodeTree. 
2. Wait `CATCHUP_API_TIMEOUT` milliseconds.
3. Select the longest `chain` from all responses.
4. If `len(chain) == CATCHUP_API_WINDOW_SIZE`, execute and insert the chain into the local Node Tree, then jump back to 1.
    1. Else if `len(chain) < CATCHUP_API_WINDOW_SIZE`, execute and insert the chain into the local Node Tree, then return with a success status.
    2. If a Node fails validation in the 'execute and insert' step, panic (this indicates that more than $f$ Participants are faulty at this moment).

### 4.4.3. Progress Mode

![A graphic with 2 rows of 4 rectangles each. The first row depicts the phases of Leader execution, the second depicts the phases of Replica execution.](./readme_assets/Phases%20of%20Progress%20Mode%20execution.png)

The *Progress Mode* works to extend the NodeTree. Starting with *BeginView*, the flow cycles between 5 states: *BeginView*, *Leader*, *Replica*, *NextLeader*, and *NewView*. This section documents Progress Mode behavior in terms of its steps. To aid with understanding, we group the steps of the Leader and Replica states in terms of high-level 'phases', as illustrated in the time-sequence diagram above.

### BeginView

1. `generic_qc = get_generic_qc()`. 
2. `view_number = max(view_number, generic_qc.view_number + 1)`
3. If `leader(view_number) == this Participant`: jump to *Leader*, else, jump to *Replica*.

### Leader 

#### Phase 1: Select the QC to justify the proposal. 
1. Read messages from *every* participant **until** all streams are empty **or** until a new QC is collected, in which case set `generic_qc` to the new QC:
    - If message is a `VOTE`:
        1. If `vote.view_number < cur_view`: discard it.
        2. If `vote.node_hash` is not in the local NodeTree: switch to *Sync Mode*.
        3. If `vote.view_number > cur_view`: discard it.
        4. Else (if `vote.view_number == cur_view`): attempt to collect it into a QC.
    - If message is a `NEW-VIEW`:
        1. If `new_view.view_number < cur_view - 1`: discard it.
        2. If `new_view.qc.view_number > generic_qc.view_number`:
           - ...and `new_view.qc` is not in the local NodeTree, switch to *Sync Mode*.
           - else: `generic_qc` = `new_view.qc`.
        3. Else: discard it.
    - Else (if message is a `PROPOSE`): discard it. 

#### Phase 2: Produce a new Node.
2. Call `create_leaf` on Application with `parent = qc.node`.

#### Phase 3: Broadcast a Proposal.
3. Broadcast a new `PROPOSE` message containing the leaf to every participant.
4. Send a `VOTE` for our own proposal to the next leader.

#### Phase 4: Wait for Replicas to finish sending out votes. 
5. Sleep for the remaining duration of the View Timeout.
6. Set `cur_view += 1` and return to *BeginView*.

### Replica 

#### Phase 1: Wait for a Proposal.
1. Read messages from the current leader **until** a matching proposal is received:
    - If message is a `PROPOSE`:
        1. If `propose.view_number < cur_view`: discard it.
        2. If `propose.node.justify.node_hash` is not in the local NodeTree: switch to *Sync Mode*.
        3. If `propose.view_number > cur_view`: discard it.
        4. Else (if `propose.view_number == cur_view`): continue.
    - If message is a `VOTE`: discard it.
    - Else (if message is a `NEW-VIEW`):
        1. If `new_view.view_number < cur_view - 1`: discard it.
        2. If `new_view.qc.view_number > get_generic_qc().view_number` and `new_view.qc` is not in the local NodeTree, switch to *Sync Mode*.
        3. Else: discard it.

#### Phase 2: Validate the proposed Node.
2. Call `validate` on Application with `node = proposal.node` and `parent = proposal.node.justify.node_hash`:
    - If Application rejects the node, jump to `NewView`.

#### Phase 3: Send a vote.
3. Send out a `VOTE` containing `node_hash == proposal.node.hash()`.

#### Phase 4: Wait for other Replicas to finish sending votes.
5. Sleep for the remaining duration of the View Timeout.
4. Set `view_number += 1` and return to *BeginView*.

### NextLeader

### NewView

This state is entered when a *View Timeout* is triggered at any point in the Engine's lifetime.

1. Send out a `NEW-VIEW` containing `view_number` and `get_generic_qc()`.
2. Set `view_number += 1` and return to *BeginView*.

### 4.4.4. View Timeouts

View timeouts determine how long the Engine Thread remains on a View before deciding that further progress at the current View is unlikely and moving onto the next View.

If View timeout is a constant and Participants' View Numbers become unsynchronized at any point in the protocol's lifetime, then Participants will never become synchronized at the same View ever again, preventing further progress.

In order to ensure that Participants eventually synchronize on the same View Number for long enough to make progress, the duration of a View timeout grows exponentially in terms of the number of consecutive views the Participant fails to insert a new node:

```rust
Timeout(cur_view, generic_qc) = 2 ** (cur_view - generic_qc.view_number)
```