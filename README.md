# HotStuff-rs 
HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
1. Guaranteed Safety and Liveness in the face of up to 1/3rd of Voting Power being Byzantine at any given moment,
2. A small API (`Executor`) for plugging in state machine-based applications like blockchains, and
3. Well-documented, 'obviously correct' source code.

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

The *Sync Mode*'s role in HotStuff-rs is exactly to synchronize the Participant Set on the two counts. Engine switches into Sync Mode when it fails to make progress (i.e., insert a new Node) in consecutive `CATCHUP_FLOW_TRIGGER` View Numbers (configurable).  

The flow proceeds in the following sequence of steps:
1. Query all Participants for a chain of length max `CATCHUP_API_WINDOW_SIZE` extending from the latest committed Node of local NodeTree. 
2. Wait `CATCHUP_API_TIMEOUT` milliseconds.
3. Select the longest `chain` from all responses.
4. If `len(chain) == CATCHUP_API_WINDOW_SIZE`, execute and insert the chain into the local Node Tree. 
4. Jump to 1.

**(Alt 3a).** If `len(chain) < CATCHUP_API_WINDOW_SIZE`, return after executing and inserting the chain into the local Node Tree (do not jump back to 1).

**(Alt 3b).** If a Node fails validation in the 'execute and insert' step, panic (this indicates that more than $f$ Participants are faulty at this moment).

### 4.4.3. Progress Mode

The *Progress Mode* works to extend the NodeTree. Starting with *BeginView*, the flow proceeds through the following states:

### BeginView

1. `generic_qc = get_generic_qc()`. 
2. Set `view_number = max(view_number, get_generic_qc() + 1)`
3. If `leader(view_number) == this Participant`, jump to *Leader*, else, jump to *Replica*.

### Leader 

1. Read a message from `ipc_manager` until all streams are empty:
    - If message is a `VOTE` and `vote.view_number == view_number`: attempt to collect it into a QC (if a QC is collected, continue to step 2).
    - If message is a `NEW-VIEW` and `new_view.qc.view_number > view_number`: switch into *Sync Mode*. 
    - Else, discard the message.
2. Set `qc` to either:
    - The `qc` collected in Step 1, or
    - (if Step 1 terminated without a QC being collected), `generic_qc`. 
3. Call `create_leaf` on Application with `parent = qc.node`.
4. Send out a new `PROPOSAL` containing the leaf.
5. Set `view_number += 1` and return to *BeginView*.


### Replica 

1. Read a `PROPOSAL` message with `proposal.view_number == view_number` from `ipc_manager` from the current leader:
    - If `proposal.node.justify.node_hash` is *not* in the local NodeTree: switch into *Sync Mode*.
    - If `proposal` does not satisfy the `SafeNode` predicate, jump to `NewView`.
2. Call `validate` on Application with `node = node` and `parent = proposal.node.justify.node_hash`:
    - If Application rejects the node, jump to `NewView`.
3. Send out a `VOTE` containing `node_hash == proposal.node.hash()`.
4. Set `view_number += 1` and return to *BeginView*.

### NewView

This state is entered when a *View Timeout* is triggered at any point in the Engine's lifetime.

1. Send out a `NEW-VIEW` containing `view_number` and `generic_qc`.
2. Set `view_number += 1` and return to *BeginView*.


### 4.4.4. View Timeouts

View timeouts determine how long the Engine Thread remains on a View before deciding that further progress at the current View is unlikely and moving onto the next View.

If View timeout is a constant and Participants' View Numbers become unsynchronized at any point in the protocol's lifetime, then Participants will never become synchronized at the same View ever again, preventing further progress.

In order to ensure that Participants eventually synchronize on the same View Number for long enough to make progress, the duration of a View timeout increases exponentially every time a timeout is triggered until some high, configurable limit defined by `MAX_TIMEOUT_EXPONENT`: 

$Timeout(n, M) = 2^{min(n, M)}$

Where $n$ is the number of consecutive views that ended with a NEW-VIEW (without progress being made), and $m$ is `MAX_TIMEOUT_EXPONENT`.
