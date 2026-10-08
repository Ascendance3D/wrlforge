import { add } from './lib/a.js';
add(1, 2);
throw new Error('module top-level failure');
