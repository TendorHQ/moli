(() => {
  const frame = document.createElement('iframe');
  document.body.append(frame);
  try {
    const other = frame.contentWindow;
    const foreignObject = other.Object.prototype;
    const getter = (Constructor, member, expected) => {
      const read = Object.getOwnPropertyDescriptor(Constructor.prototype, member).get;
      return value => read.call(value) === expected;
    };
    const cases = [
      ['DOMParser', [], value =>
        DOMParser.prototype.parseFromString.call(value, '<p>native</p>', 'text/html').nodeType === 9],
      ['RTCIceCandidate', [{candidate: '', sdpMid: '0'}], getter(RTCIceCandidate, 'candidate', '')],
      ['RTCSessionDescription', [{type: 'offer'}], getter(RTCSessionDescription, 'type', 'offer')],
      ['RTCPeerConnectionIceEvent', ['icecandidate'], getter(RTCPeerConnectionIceEvent, 'candidate', null)],
      ['RTCPeerConnectionIceErrorEvent', ['icecandidateerror', {errorCode: 701}],
        getter(RTCPeerConnectionIceErrorEvent, 'errorCode', 701)],
    ].map(([name, args, brand]) => ({
      name, args, brand, Constructor: window[name], foreignPrototype: other[name].prototype,
    }));
    const foreign = other.Function('');
    foreign.prototype = 7;
    const proxy = new Proxy(foreign, {getPrototypeOf() {return Function.prototype;}});
    const bound = Function.prototype.bind.call(proxy, null);
    const targets = [
      ['foreign', foreign],
      ['boundProxy', bound],
      ['proxyBound', new Proxy(bound, {})],
      ['nestedBound', Function.prototype.bind.call(new Proxy(bound, {}), null)],
    ];
    // Defaults must come from realm intrinsics, even after replacing globals.
    other.Object = function PoisonedObject() {throw Error('author Object');};
    for (const {name} of cases)
      other[name] = function PoisonedInterface() {throw Error('author interface');};
    const fallbacks = [];
    for (const [target, callable] of targets) {
      for (const {name, Constructor, args, brand, foreignPrototype} of cases) {
        let reads = 0;
        const newTarget = new Proxy(callable, {get(object, key, receiver) {
          if (key === 'prototype') ++reads;
          return Reflect.get(object, key, receiver);
        }});
        const value = Reflect.construct(Constructor, args, newTarget);
        const prototype = Object.getPrototypeOf(value);
        fallbacks.push({name, target, prototypeReads: reads,
          targetInterface: prototype === foreignPrototype,
          targetObject: prototype === foreignObject, nativeBrand: brand(value)});
      }
    }
    const revoked = [];
    const channelCase = {
      name: 'BroadcastChannel', Constructor: BroadcastChannel,
      args: ['revoked-constructor'], brand: getter(BroadcastChannel, 'name', 'revoked-constructor'),
    };
    for (const {name, Constructor, args, brand} of [...cases, channelCase]) {
      const prototype = Object.create(null);
      const pair = Proxy.revocable(function() {}, {get(object, key, receiver) {
        if (key === 'prototype') {pair.revoke(); return prototype;}
        return Reflect.get(object, key, receiver);
      }});
      const value = Reflect.construct(Constructor, args, pair.proxy);
      revoked.push({name, customPrototype: Object.getPrototypeOf(value) === prototype,
        nativeBrand: brand(value)});
      if (name === 'BroadcastChannel') BroadcastChannel.prototype.close.call(value);
    }
    return {fallbacks, revoked};
  } finally {
    frame.remove();
  }
})()
