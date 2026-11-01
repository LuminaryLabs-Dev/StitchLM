// Pass a checkout of the pinned NexusEngine. This is test tooling, not product code.
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
import {readFileSync} from 'node:fs';
const engine=process.argv[2], actualPath=process.argv[3];
if(!engine||!actualPath)throw new Error('engine checkout and Rust output JSON required');
const {createInferenceRequest,createInferenceResult}=await import(pathToFileURL(`${engine}/src/core-domains/inference/contract/index.js`));
const actual=JSON.parse(readFileSync(actualPath,'utf8'));
assert.deepEqual(actual.request,createInferenceRequest({id:'test-request',modelId:'model',messages:[{role:'user',content:'hello'}],tokenBudget:8}));
assert.deepEqual(actual.result,createInferenceResult({requestId:'test-request',modelId:'model',output:'hello',usage:{output_tokens:2},providerId:'stitchlm-cpu',traceId:'test-request'}));
console.log('Pinned Nexus inference request/result parity passed');
