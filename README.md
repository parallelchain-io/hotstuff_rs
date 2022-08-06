# HotStuff-rs 
HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
1. Guaranteed Safety and Liveness in the face of up to 1/3rd of Voting Power being Byzantine at any given moment,
2. A small API (`Executor`) for plugging in state machine-based applications like blockchains, and
3. Well-documented, 'obviously correct' source code.

## 1. Reading this document
1. If you just want to *use* HotStuff-rs in your application: read Section 2.
2. If you want to *understand* how HotStuff works: read the HotStuff [paper](./readme_assets/HotStuff%20paper.pdf).
3. If you want to *contribute* to HotStuff-rs' development: read Sections 2 and 3, then read the (specification)[./SPECIFICATION.md]. 

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

#### 3.1.1. CatchUp Flow

#### 3.1.2. Progress Flow

### 3.2. Consensus TCP API

The Consensus TCP API carries `PROPOSE`, `VOTE`, and `NEW-VIEW` messages between Replicas. When all Nodes are up-to-date, this is the only HotStuff-rs network API that will see any traffic. 

### 3.3. NodeTree HTTP API

The NodeTree HTTP API serves requests for Nodes in NodeTree. This is used by out-of-date Replicas to catch up to the head of the NodeTree.  

### 3.4. Connection Manager

### 3.5. Your Application

Anything that *you*, the HotStuff-rs user, can imagine. The Actor thread expects it to implement a trait `Application`.

