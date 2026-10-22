(() => {
  const stringify = Function.prototype.toString;
  const constructors = [
    ['DOMParser', []],
    ['RTCIceCandidate', [{candidate: '', sdpMid: '0'}]],
    ['RTCSessionDescription', [{type: 'offer', sdp: ''}]],
  ].map(([name, args]) => {
    const Constructor = globalThis[name];
    const instance = new Constructor(...args);
    class Derived extends Constructor {}
    const derived = new Derived(...args);
    const reflected = Reflect.construct(Constructor, args, Derived);
    return {
      name,
      source: stringify.call(Constructor),
      instanceOf: instance instanceof Constructor,
      instancePrototype: Object.getPrototypeOf(instance) === Constructor.prototype,
      instanceConstructor: instance.constructor === Constructor,
      prototypeConstructor: Constructor.prototype.constructor === Constructor,
      subclassInstanceOf: derived instanceof Constructor && derived instanceof Derived,
      subclassPrototype: Object.getPrototypeOf(derived) === Derived.prototype,
      subclassConstructor: derived.constructor === Derived,
      subclassParent: Object.getPrototypeOf(Derived) === Constructor,
      reflectedInstanceOf: reflected instanceof Constructor && reflected instanceof Derived,
      reflectedPrototype: Object.getPrototypeOf(reflected) === Derived.prototype,
      reflectedConstructor: reflected.constructor === Derived,
    };
  });
  return {
    constructors,
    toStringUnchanged: stringify === Function.prototype.toString,
    builtinSource: stringify.call(Object),
    authoredSource: stringify.call(function authored() {}),
    callableProxySource: stringify.call(new Proxy(function authored() {}, {})),
  };
})()
