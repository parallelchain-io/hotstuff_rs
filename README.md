# HotStuff-rs 
HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
1. Guaranteed Safety and Liveness in the face of up to 1/3rd of Voting Power being Byzantine at any given moment,
2. A small API (`Executor`) for plugging in state machine-based applications like blockchains, and
3. Well-documented, 'obviously correct' source code.

## The HotStuff Consensus Protocol

HotStuff works by building a 'NodeTree': a directed acyclic graph of Nodes. Node is a structure with a `command` field which applications are free to populate with arbitrary byte-arrays. In consensus algorithm literature, we typically talk of consensus algorithms as maintaining state machines that change their internal states in response to commands, hence, the choice of terminology.

HotStuff guarantees that committed Nodes are *immutable*. That is, they can never be *un*-committed as long as at least a supermajority of voting power faithfully execute the protocol. This guarantee enables applications to make hard-to-reverse actions with confidence. 

![A graphic depicting a Tree (DAG) of nodes. Nodes are colored depending on how many confirmations they have.](./readme_assets/NodeTree.png "NodeTree")

A Node becomes *committed* the instant its third confirmation is written into the NodeTree. A confirmation for a Node `A` is another Node `B` such that there is path between `B` to `A`.

The choice of third confirmation to define commitment--as opposed to first or second--is not arbitrary. HotStuff's safety and liveness properties actually hinge upon on this condition. If you really want to understand why this is the case, you should read the [paper](./readme_assets/HotStuff%20paper.pdf). To summarize:

1. Classic BFT consensus algorithms such as PBFT require only 2 confirmations for commitment, but this comes at the cost of expensive leader-replacement flows.
2. Tendermint require only 2 confirmations for commitment and has a simple leader-replacement flow, but needs an explicit 'wait-for-N seconds' step to guarantee liveness.

HotStuff is the first consensus algorithm with a simple leader-replacement algorithm that does not have a 'wait-for-N seconds' step, and thus can make progress as fast as network latency allows.

## Major Components

![A graphic depicting the set of relationships between HotStuff-rs' components](./readme_assets/Components.png "Components")

### Actor Thread

The Actor Thread drives consensus forward by participating in the creation of new Nodes.

Responsibilities:
1. Create and propose new nodes (the 'Leader flow') in collaboration with your Application.
2. Vote on newly proposed nodes (the 'Replica flow').
3. Decide how long to stay in a current view.

### Listener Thread 

The Listener Thread ensures that the Actor Thread has all the information it needs to drive consensus forward.

Responsibilities:
1. Serve as the *exclusive* point of entry for `PROPOSE`, `VOTE`, and `NEW-VIEW` protocol messsages.
2. Identify messages that are 'from the *past*' (outdated), e.g., `PROPOSALs` that build on abandoned branches, and discard them.
3. Identify messages that are 'from the *future*', e.g., `VOTEs` on Nodes that are *not yet* in NodeTree, and get missing nodes using the NodeTree HTTP API (Client side) and insert them into the **NodeTree**.
4. Insert non-outdated `VOTEs` and `PROPOSALs` into **VotesQueue** and **ProposalsQueue** for eventual processing by the **Actor Thread**

### NodeTree

The NodeTree maintains a constantly growing DAG of Nodes. As new Nodes are inserted into NodeTree, existing Nodes accumulate more and more confirmations. When an insertion causes an existing Node accumulates 3 confirmations, NodeTree automatically applies its World State mutations into persistent storage and abandons conflicting branches.

### ProposalsQueue

A mapping between View Number and the `PROPOSAL` that the Listener Thread has received for that View Number (if it has received such a proposal). When the Actor Thread takes the proposal for a particular View Number, ProposalsQueue automatically drops all proposals for smaller View Numbers.

### VotesQueue

A mapping between View Number and the set of `VOTEs` that the Listener Thread has received for that View Number. When the Actor Thread takes the proposal for a particular View Number, VotesQueue automatically drops all votes for smaller View Numbers.

### Consensus TCP API

The Consensus TCP API carries `PROPOSE`, `VOTE`, and `NEW-VIEW` messages between Replicas. When all Nodes are up-to-date, this is the only HotStuff-rs network API that will see any traffic. 

### NodeTree HTTP API

The NodeTree HTTP API serves requests for Nodes in NodeTree. It is used by out-of-date Replicas to catch up to the head of the NodeTree.  

### Your Application

Anything that *you*, the HotStuff-rs user, can imagine. The Actor thread expects it to implement a trait `Application`.

## Protocol Description 

### Actor sequence flow 

If, at any point in this sequence flow, a View timeout is triggered, jump to the **View timeout flow**.

#### Setup flow

1. Get `prepare_qc` from the NodeTree.
2. Compute the `leader` of View PrepareQC.view_number. 
3. If `leader` is this HotStuff-rs Replica, jump to Leader flow, else, jump to **Replica flow**. 

#### Leader flow 

1. Take a quorum of `votes` from VotesQueue.
2. Aggregate `votes` into a `qc`. 
3. Call Application to produce a new `command` extending qc.node.
4. Create a new `node` containing `command` and `qc`.
5. Insert `node` into the local NodeTree.
6. Send out a `PROPOSAL` `proposal` containing `node` to all replicas.
7. Send out a `vote` for `proposal` to the leader of the next view.
8. Jump to **Setup flow**.

#### Replica flow

1. Take a `proposal` from ProposalQueue. 
2. Call Application to execute `proposal.node.command`, if Application approves, continue, if Application disapproves, jump to **View timeout flow**.
3. Insert `proposal.node` into the local NodeTree.
4. Send out a `VOTE` for `proposal` to the leader of the next view.
5. Jump to **Setup flow**.

#### View timeout flow

1. Send out a `NEW-VIEW` containing  `prepare_qc` to the leader of the next view.
2. Jump to **Setup flow**.

### View timeouts
### Listener sequence flow 

### Consensus API Types

#### **NODE(parent, command, justify)**
|Field |Type |Description |
|---   |---  |---         |
|parent |`NodeHash` | |
|command |`Vec<u8>` | |
|justify |`QC` | |

##### **NodeHash**
SHA256Hash over `parent ++ command ++ justify ++ view_number`.

#### **QC(view_num, node, sigs)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node |`NodeHash` | |
|sigs |`Vec<(PublicAddress, Signature)>` | |

#### **Sig(public_address, signature)**
|Field |Type |Description |
|---   |---  |---         |
|public_address |`SHA256Hash` | |
|signature |`Ed25519Signature`

### Consensus API Messages

#### **PROPOSE(view_number, node)**
|Field |Type |Description |
|---   |---  |---         |
|view_number |`u64` | |
|node |`Node` | |

#### **VOTE(view_num, node, sig)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node |`NodeHash` | |
|sig |`Sig` | |

#### **NEW-VIEW(view_num, high_qc)**
|Field |Type |Description |
|---   |---  |---         |
|view_num|`u64` | |
|high_qc |`QC` | |

