/* react/jsx-runtime@19.2.7 — vendored by resources/js/react/build.mjs */
var p=Object.create;var n=Object.defineProperty;var v=Object.getOwnPropertyDescriptor;var k=Object.getOwnPropertyNames;var T=Object.getPrototypeOf,_=Object.prototype.hasOwnProperty;var x=(e,r)=>()=>(r||e((r={exports:{}}).exports,r),r.exports);var a=(e,r,t,o)=>{if(r&&typeof r=="object"||typeof r=="function")for(let s of k(r))!_.call(e,s)&&s!==t&&n(e,s,{get:()=>r[s],enumerable:!(o=v(r,s))||o.enumerable});return e};var f=(e,r,t)=>(t=e!=null?p(T(e)):{},a(r||!e||!e.__esModule?n(t,"default",{value:e,enumerable:!0}):t,e));var d=x(l=>{"use strict";/**
 * @license React
 * react-jsx-runtime.production.js
 *
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */var m=Symbol.for("react.transitional.element"),A=Symbol.for("react.fragment");function E(e,r,t){var o=null;if(t!==void 0&&(o=""+t),r.key!==void 0&&(o=""+r.key),"key"in r){t={};for(var s in r)s!=="key"&&(t[s]=r[s])}else t=r;return r=t.ref,{$$typeof:m,type:e,key:o,ref:r!==void 0?r:null,props:t}}l.Fragment=A;l.jsx=E;l.jsxs=E});var j=x((R,i)=>{"use strict";i.exports=d()});var u=f(j()),c=u.default,{Fragment:q,jsx:C,jsxs:M}=u.default;export{q as Fragment,c as default,C as jsx,M as jsxs};
