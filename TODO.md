## Non-exhaustive list of ideas, missing features, and bugs that could be fixed.

# Ideas

- Managed main loop for external loop applications that will handle swapchain creation and management, drawing of the interface, and merging of the user created graphics and the interface.
- Merge of input and bin hook systems to provide better interoperability and performance.
- Investigate synchronization of input handling to better improve latency and reporting intervals. 
- Allow use of custom fonts in Interface instead of just the included one.

# Missing Implementations

- Deletion of Atlas images is currently not implemented. This would include reclaiming space of deleted images, along with defragmenting current consumed space.
- Bins currently lack any form of horizontal overflow including, but not limited to overflow calculations, cutting off overflowing content, and scrolling of content horizontally.
- Implement borders on Bins when a radius is present.

# Reworks of Existing API's

- ItfRenderer currently consumes `AutoCommandBufferBuilder` and returns one back. It would be more ideal to instead return a `SecondaryCommandBuffer` where the user can the use `execute_commands()` now that the method is implemented.
