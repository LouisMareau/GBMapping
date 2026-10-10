// Reads a screenshot's pixels: the PNGs Chrome writes (8 bits a channel, RGB or RGBA,
// not interlaced), with nothing but Node's zlib.
import { inflateSync } from 'node:zlib';

export function decodePng(buffer) {
  let at = 8, width = 0, height = 0, channels = 0;
  const packed = [];
  while (at < buffer.length) {
    const length = buffer.readUInt32BE(at);
    const kind = buffer.toString('latin1', at + 4, at + 8);
    const body = buffer.subarray(at + 8, at + 8 + length);
    if (kind === 'IHDR') {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      channels = { 2: 3, 6: 4 }[body[9]];
      if (body[8] !== 8 || !channels || body[12] !== 0) throw new Error('not a PNG this reads');
    } else if (kind === 'IDAT') {
      packed.push(body);
    }
    at += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(packed));
  const stride = width * channels;
  const data = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = y * stride;
    for (let i = 0; i < stride; i++) {
      const left = i >= channels ? data[line + i - channels] : 0;
      const up = y ? data[line - stride + i] : 0;
      const corner = y && i >= channels ? data[line - stride + i - channels] : 0;
      let predicted = 0;
      if (filter === 1) predicted = left;
      else if (filter === 2) predicted = up;
      else if (filter === 3) predicted = (left + up) >> 1;
      else if (filter === 4) {
        const p = left + up - corner;
        const [a, b, c] = [Math.abs(p - left), Math.abs(p - up), Math.abs(p - corner)];
        predicted = a <= b && a <= c ? left : b <= c ? up : corner;
      }
      data[line + i] = (raw[y * (stride + 1) + 1 + i] + predicted) & 255;
    }
  }
  return { width, height, channels, data };
}

/// The share of the pixels that differ between two rectangles of a picture, each
/// `width` x `height` from its own top left corner
export function difference(image, [ax, ay], [bx, by], width, height) {
  let differing = 0;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const a = ((ay + y) * image.width + ax + x) * image.channels;
      const b = ((by + y) * image.width + bx + x) * image.channels;
      for (let c = 0; c < 3; c++) {
        if (Math.abs(image.data[a + c] - image.data[b + c]) > 16) {
          differing++;
          break;
        }
      }
    }
  }
  return differing / (width * height);
}
