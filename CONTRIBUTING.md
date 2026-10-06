# Contributing to Steel

We welcome contributions to Steel! By contributing, you help us make Steel the best Minecraft server it can be. Please take a moment to review this document to understand how to contribute effectively.

## Before You Start

### Discuss Major Changes

Before implementing any major changes or new features, please create a post in the [#feature-discussion](https://canary.discord.com/channels/1428487339759370322/1429074039015473272) channel on our [Discord](https://discord.gg/MwChEHnAbh) server. This allows for discussion of potential problems, alternative solutions, and overall alignment with the project's direction.

### Minor changes

Don't PR changes that have no impact on code foundation/structure, unless paired with a partial or full implementation. This ensures that stale code will be minimized.

### Consider Future Use Cases

When optimizing code or removing "unnecessary" vanilla Minecraft elements, always consider their potential use cases in future implementations. This also applies when choosing which crate to place new functionality into; think about modularity and future extensibility.

### Use Parchment Mappings

Please stick to Parchment mapping names most of the time for consistency and easier collaboration.

### AI policy

We allow the use of generative AI models such as LLM's for the generation of code in our project. However, we will not tolerate "vibe coding", meaning:
1. Do not overuse AI - You should understand the code that has been generated.
2. Re-read, check and correct the generated code to fit our coding guidelines and reduce sub-optimal implementation, as well as poorly readable doc-comments.


Thank you for contributing to Steel!
