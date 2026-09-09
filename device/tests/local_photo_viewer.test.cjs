const {test} = require('node:test');
const assert = require('node:assert/strict');
const {readPhotoExif} = require('../src/local_photo_viewer.js');
function fixture(little) {
  const buffer = new ArrayBuffer(100), v = new DataView(buffer);
  v.setUint16(0,0xffd8); v.setUint16(2,0xffe1); v.setUint16(4,94);
  v.setUint32(6,0x45786966); v.setUint16(10,0);
  const base=12;
  v.setUint16(base,little?0x4949:0x4d4d);
  const u16=(p,x)=>v.setUint16(base+p,x,little),u32=(p,x)=>v.setUint32(base+p,x,little);
  u16(2,42);u32(4,8);u16(8,1);
  u16(10,0x8769);u16(12,4);u32(14,1);u32(18,26);
  u16(26,3);
  u16(28,0x829a);u16(30,5);u32(32,1);u32(36,68);
  u16(40,0x8827);u16(42,3);u32(44,1);u16(48,200);
  u16(52,0x9206);u16(54,5);u32(56,1);u32(60,76);
  u32(68,1);u32(72,200);u32(76,2);u32(80,5);
  return buffer;
}
for (const little of [true,false]) test(`reads exposure, ISO and distance (${little?'little':'big'} endian)`,()=>{
  assert.deepEqual(readPhotoExif(fixture(little)),{'Exposure (s)':0.005,ISO:200,'Reported focus distance (m)':0.4});
});
test('absent EXIF stays absent',()=>assert.deepEqual(readPhotoExif(Uint8Array.from([255,216,255,217]).buffer),{}));
test('truncated inputs never throw',()=>{
  const valid=fixture(true);
  for(let i=0;i<valid.byteLength;i++) assert.doesNotThrow(()=>readPhotoExif(valid.slice(0,i)));
});
test('invalid EXIF offsets and zero denominators are ignored',()=>{
  const b=fixture(true),v=new DataView(b);v.setUint32(12+36,0xffffff00,true);
  v.setUint32(12+80,0,true);
  assert.deepEqual(readPhotoExif(b),{ISO:200});
});
