# Official Bluetooth storage protocol

Source supplied by user: https://docin.divoom-gz.com/web/#/5/23. The SPA renders correctly in the in-app browser. Page23 itself is Contact Divoom; Bluetooth > light contains the relevant protocol pages.

## 0x8C indexed user-defined transfer

https://docin.divoom-gz.com/web/#/5/296

- Start control0: file size u32LE, then destination index u8 starting0. Wait for device response. Documentation says response uses Set user gif (0xB1).
- Data control1: total file length u32LE, chunk ID u16LE starting0, at most256 file bytes.
- Retransmission response: main0x04, cmd0x8C, ack0x55, control1, requested chunk ID u16LE.
- Terminate control2: no further data.

## 0xB1 user GIF saving

https://docin.divoom-gz.com/web/#/5/294

- Control0 starts saving, wait for response; control1 transmits complete frame; control2 ends transmission.
- Start/end Data0=0 normalimage,1LEDeditor,2sandpainting,3scrollanimation.
- Data control1: current length u16LE and complete encoded image frame.
- Response main0x04, cmd0xB1, ack0x55.

## Related commands

https://docin.divoom-gz.com/web/#/5/295: 0xB6FF queries current saved-item count. Every other value deletes an item numbered from1; do not use deletion for exploratory queries.

https://docin.divoom-gz.com/web/#/5/293: 0x8B is ordinary animated-data transfer. Start0+sizeu32LE waits for response main04,cmd8B,ack55,control0,ready1 or refusal0. Data1+sizeu32LE+chunku16LE+up to256bytes; terminate2. No destination index.

https://docin.divoom-gz.com/web/#/5/297: 0x8D targets64largecanvas, not this32x32display. Do not conflate.

https://docin.divoom-gz.com/web/#/5/298: 0x8E enumerates IDs for64customimages, not confirmed on Max.

https://docin.divoom-gz.com/web/#/5/289 and /290: image/animation data is the output of divoom_image_encode_encode_pic. No actual binary frame specification appears on those pages. Existing Go encoder already reproduces native AAframes and two Maxflagframes05/06 for32x32images.

Current status before implementation/probing: indexed-save payload shape is documented, but Max acceptance, exact saved-file encoding, and save completion remain unverified. Prior normal0x49upload did not update selectedtab1. User permits stored artwork to be wiped if necessary. Do not guess reset/firmware opcodes.

## Results on this Pixoo-Max

0x8C accepts the existing native AA animation stream including the Max flag frames. Start response is `01 05 00 04 B1 55 0F 01 02`. After all chunks and termination, the observed completion is `01 1B 00 04 46 55 05 00 01 FF 50 00 64 00 01 03 64 00 FF 50 00 02 01 00 00 00 37 08 6C 04 02`. The 46 response reports custom mode05. There is no second B1 response on this unit.

Index0 save was followed by stored2 then stored1 without upload. Owner confirmed the PR animation returned. Index1 save was followed by stored2 without upload, and the owner confirmed the blue build animation. Index2 save was followed by stored1 then stored3 without upload; owner confirmed the red X animation returned. The diagnostic's original finish-ACK timeout was an overly strict completion assumption, not evidence the save failed. Captured transcripts are save-8c-first.log, save-8c-slot2.log, and save-8c-slot3.log.

B6FF reports zero before and after successful saves. It is not a useful success check for this storage path. Power-cycle retention remains untested.
