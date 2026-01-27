/*! flex-sdk-web v0.2.1 | (c) CyberSource 2017 */ ! function(e, t) {
    "object" == typeof exports && "undefined" != typeof module ? module.exports = t() : "function" == typeof define && define.amd ? define(t) : e.FLEX = t()
}(this, function() {
    "use strict";

    function e() {
        return "undefined" != typeof XMLHttpRequest && "withCredentials" in new XMLHttpRequest
    }

    function t() {
        return "undefined" != typeof XDomainRequest
    }

    function r() {
        return e() || t()
    }

    function n(e) {
        var t = void 0 === e ? "undefined" : v(e);
        return null != e && ("object" === t || "function" === t)
    }

    function o(e) {
        var t = n(e) ? Object.prototype.toString.call(e) : "";
        return "[object Function]" === t || "[object GeneratorFunction]" === t
    }

    function i(e) {
        return JSON.parse(JSON.stringify(e))
    }

    function a(e) {
        for (var t = new ArrayBuffer(e.length), r = new Uint8Array(t), n = 0, o = e.length; n < o; n += 1) r[n] = e.charCodeAt(n);
        return t
    }

    function u() {
        return window.crypto && window.crypto.subtle && "function" == typeof window.crypto.subtle.importKey && "function" == typeof window.crypto.subtle.encrypt
    }

    function c(e, t) {
        var r = i(e);
        return /Edge/.test(window.navigator.userAgent) && delete r.use, window.crypto.subtle.importKey("jwk", r, t, !1, ["encrypt"])
    }

    function s(e, t, r) {
        return window.crypto.subtle.encrypt(e, t, a(r)).then(b)
    }

    function f(e) {
        var t = void 0;
        switch ((e.encryptionType || "").toLowerCase()) {
            case "none":
                t = A();
                break;
            case "rsaoaep":
                t = S(e);
                break;
            case "rsaoaep256":
                t = k(e);
                break;
            default:
                    t = k(e);
                // throw new Error('Unsupported encryption type "' + e.encryptionType + '"')
        }
        return {
            encrypt: function(e) {
                return t.encrypt(e)
            }
        }
    }

    function p(e) {
        var t = arguments.length > 1 && void 0 !== arguments[1] ? arguments[1] : 200,
            r = void 0;
        try {
            r = JSON.parse(e)
        } catch (t) {
            r = e
        }
        return t >= 200 && t < 400 && null != r.token ? r : {
            error: r
        }
    }

    function l(e, t) {
        var r = {
            keyId: e,
            cardInfo: {
                cardNumber: t.cardNumber,
                cardType: t.cardType
            }
        };
        return t.cardExpirationMonth && (r.cardInfo.cardExpirationMonth = t.cardExpirationMonth), t.cardExpirationYear && (r.cardInfo.cardExpirationYear = t.cardExpirationYear), r
    }

    function y(e, t, r) {
        var n = new XMLHttpRequest;
        n.open("POST", e, !0), n.setRequestHeader("Content-Type", "application/json; charset=utf-8"), n.timeout = E, n.ontimeout = function() {
            return r(p("Request has timed out"))
        }, n.onerror = function() {
            return r(p(n.responseText, n.status))
        }, n.onload = function() {
            return r(p(n.responseText, n.status))
        }, n.send(JSON.stringify(t))
    }

    function h(e, t, r) {
        var n = new XDomainRequest;
        n.timeout = E, n.onprogress = function() {}, n.ontimeout = function() {
            r(p("Request has timed out"))
        }, n.onerror = function() {
            r(p("Detailed error response unavailable in this browser"))
        }, n.onload = function() {
            var e = void 0;
            try {
                e = JSON.parse(n.responseText)
            } catch (t) {
                e = {
                    error: n.responseText
                }
            }
            r(p(e))
        }, n.open("POST", e), n.send(JSON.stringify(t))
    }

    function d(r, n, o, i) {
        var a = l(n, o);
        if (e()) y(r, a, i);
        else {
            if (!t()) throw new Error("Browser does not support CORS requests.");
            h(r, a, i)
        }
    }
    function jmcrypt(e, r) {
        n = e.cardInfo || {};
        f(e).encrypt(n.cardNumber).then(function(o) {
            return r(o)
        }).catch(function(e) {
            throw e
        })
    }
    function g(e, t) {
        if (!o(t)) throw new Error("responseHandler is not a function");
        var r = !0 === e.production ? w.prod : w.test,
            n = e.cardInfo || {};
        if (n.cardNumber = n.cardNumber ? n.cardNumber.replace(/\D/g, "") : "", n.cardNumber.length < 1) return delete n.cardNumber, void d(r, e.kid, n, t);
        f(e).encrypt(n.cardNumber).then(function(o) {
            n.cardNumber = o, d(r, e.kid, n, t)
        }).catch(function(e) {
            throw e
        })
    }
    var w = {
            test: "https://testflex.cybersource.com/cybersource/flex/v1/tokens",
            prod: "https://flex.cybersource.com/cybersource/flex/v1/tokens"
        },
        m = "undefined" != typeof window ? window : "undefined" != typeof global ? global : "undefined" != typeof self ? self : {},
        v = "function" == typeof Symbol && "symbol" == typeof Symbol.iterator ? function(e) {
            return typeof e
        } : function(e) {
            return e && "function" == typeof Symbol && e.constructor === Symbol && e !== Symbol.prototype ? "symbol" : typeof e
        };
    ! function(e, t) {
        t = {
            exports: {}
        }, e(t, t.exports), t.exports
    }(function(e) {
        ! function(t, r, n) {
            r[t] = r[t] || n(), e.exports && (e.exports = r[t])
        }("Promise", m, function() {
            function e(e, t) {
                l.add(e, t), p || (p = h(l.drain))
            }

            function t(e) {
                var t, r = void 0 === e ? "undefined" : v(e);
                return null == e || "object" != r && "function" != r || (t = e.then), "function" == typeof t && t
            }

            function r() {
                for (var e = 0; e < this.chain.length; e++) n(this, 1 === this.state ? this.chain[e].success : this.chain[e].failure, this.chain[e]);
                this.chain.length = 0
            }

            function n(e, r, n) {
                var o, i;
                try {
                    !1 === r ? n.reject(e.msg) : (o = !0 === r ? e.msg : r.call(void 0, e.msg), o === n.promise ? n.reject(TypeError("Promise-chain cycle")) : (i = t(o)) ? i.call(o, n.resolve, n.reject) : n.resolve(o))
                } catch (e) {
                    n.reject(e)
                }
            }

            function o(n) {
                var a, c = this;
                if (!c.triggered) {
                    c.triggered = !0, c.def && (c = c.def);
                    try {
                        (a = t(n)) ? e(function() {
                            var e = new u(c);
                            try {
                                a.call(n, function() {
                                    o.apply(e, arguments)
                                }, function() {
                                    i.apply(e, arguments)
                                })
                            } catch (t) {
                                i.call(e, t)
                            }
                        }): (c.msg = n, c.state = 1, c.chain.length > 0 && e(r, c))
                    } catch (e) {
                        i.call(new u(c), e)
                    }
                }
            }

            function i(t) {
                var n = this;
                n.triggered || (n.triggered = !0, n.def && (n = n.def), n.msg = t, n.state = 2, n.chain.length > 0 && e(r, n))
            }

            function a(e, t, r, n) {
                for (var o = 0; o < t.length; o++) ! function(o) {
                    e.resolve(t[o]).then(function(e) {
                        r(o, e)
                    }, n)
                }(o)
            }

            function u(e) {
                this.def = e, this.triggered = !1
            }

            function c(e) {
                this.promise = e, this.state = 0, this.triggered = !1, this.chain = [], this.msg = void 0
            }

            function s(t) {
                if ("function" != typeof t) throw TypeError("Not a function");
                if (0 !== this.__NPO__) throw TypeError("Not a promise");
                this.__NPO__ = 1;
                var n = new c(this);
                this.then = function(t, o) {
                    var i = {
                        success: "function" != typeof t || t,
                        failure: "function" == typeof o && o
                    };
                    return i.promise = new this.constructor(function(e, t) {
                        if ("function" != typeof e || "function" != typeof t) throw TypeError("Not a function");
                        i.resolve = e, i.reject = t
                    }), n.chain.push(i), 0 !== n.state && e(r, n), i.promise
                }, this.catch = function(e) {
                    return this.then(void 0, e)
                };
                try {
                    t.call(void 0, function(e) {
                        o.call(n, e)
                    }, function(e) {
                        i.call(n, e)
                    })
                } catch (e) {
                    i.call(n, e)
                }
            }
            var f, p, l, y = Object.prototype.toString,
                h = "undefined" != typeof setImmediate ? function(e) {
                    return setImmediate(e)
                } : setTimeout;
            try {
                Object.defineProperty({}, "x", {}), f = function(e, t, r, n) {
                    return Object.defineProperty(e, t, {
                        value: r,
                        writable: !0,
                        configurable: !1 !== n
                    })
                }
            } catch (e) {
                f = function(e, t, r) {
                    return e[t] = r, e
                }
            }
            l = function() {
                function e(e, t) {
                    this.fn = e, this.self = t, this.next = void 0
                }
                var t, r, n;
                return {
                    add: function(o, i) {
                        n = new e(o, i), r ? r.next = n : t = n, r = n, n = void 0
                    },
                    drain: function() {
                        var e = t;
                        for (t = r = p = void 0; e;) e.fn.call(e.self), e = e.next
                    }
                }
            }();
            var d = f({}, "constructor", s, !1);
            return s.prototype = d, f(d, "__NPO__", 0, !1), f(s, "resolve", function(e) {
                var t = this;
                return e && "object" == (void 0 === e ? "undefined" : v(e)) && 1 === e.__NPO__ ? e : new t(function(t, r) {
                    if ("function" != typeof t || "function" != typeof r) throw TypeError("Not a function");
                    t(e)
                })
            }), f(s, "reject", function(e) {
                return new this(function(t, r) {
                    if ("function" != typeof t || "function" != typeof r) throw TypeError("Not a function");
                    r(e)
                })
            }), f(s, "all", function(e) {
                var t = this;
                return "[object Array]" != y.call(e) ? t.reject(TypeError("Not an array")) : 0 === e.length ? t.resolve([]) : new t(function(r, n) {
                    if ("function" != typeof r || "function" != typeof n) throw TypeError("Not a function");
                    var o = e.length,
                        i = Array(o),
                        u = 0;
                    a(t, e, function(e, t) {
                        i[e] = t, ++u === o && r(i)
                    }, n)
                })
            }), f(s, "race", function(e) {
                var t = this;
                return "[object Array]" != y.call(e) ? t.reject(TypeError("Not an array")) : new t(function(r, n) {
                    if ("function" != typeof r || "function" != typeof n) throw TypeError("Not a function");
                    a(t, e, function(e, t) {
                        r(t)
                    }, n)
                })
            }), s
        })
    });
    ! function(e) {
        function t(e) {
            return btoa(e).replace(/\=+$/, "").replace(/\+/g, "-").replace(/\//g, "_")
        }

        function r(e) {
            return e += "===", e = e.slice(0, -e.length % 4), atob(e.replace(/-/g, "+").replace(/_/g, "/"))
        }

        function n(e) {
            for (var t = new Uint8Array(e.length), r = 0; r < e.length; r++) t[r] = e.charCodeAt(r);
            return t
        }

        function o(e) {
            return e instanceof ArrayBuffer && (e = new Uint8Array(e)), String.fromCharCode.apply(String, e)
        }

        function i(e) {
            var t = {
                name: (e.name || e || "").toUpperCase().replace("V", "v")
            };
            switch (t.name) {
                case "SHA-1":
                case "SHA-256":
                case "SHA-384":
                case "SHA-512":
                    break;
                case "AES-CBC":
                case "AES-GCM":
                case "AES-KW":
                    e.length && (t.length = e.length);
                    break;
                case "HMAC":
                    e.hash && (t.hash = i(e.hash)), e.length && (t.length = e.length);
                    break;
                case "RSAES-PKCS1-v1_5":
                    e.publicExponent && (t.publicExponent = new Uint8Array(e.publicExponent)), e.modulusLength && (t.modulusLength = e.modulusLength);
                    break;
                case "RSASSA-PKCS1-v1_5":
                case "RSA-OAEP":
                    e.hash && (t.hash = i(e.hash)), e.publicExponent && (t.publicExponent = new Uint8Array(e.publicExponent)), e.modulusLength && (t.modulusLength = e.modulusLength);
                    break;
                default:
                    throw new SyntaxError("Bad algorithm name")
            }
            return t
        }

        function a(e) {
            return {
                HMAC: {
                    "SHA-1": "HS1",
                    "SHA-256": "HS256",
                    "SHA-384": "HS384",
                    "SHA-512": "HS512"
                },
                "RSASSA-PKCS1-v1_5": {
                    "SHA-1": "RS1",
                    "SHA-256": "RS256",
                    "SHA-384": "RS384",
                    "SHA-512": "RS512"
                },
                "RSAES-PKCS1-v1_5": {
                    "": "RSA1_5"
                },
                "RSA-OAEP": {
                    "SHA-1": "RSA-OAEP",
                    "SHA-256": "RSA-OAEP-256"
                },
                "AES-KW": {
                    128: "A128KW",
                    192: "A192KW",
                    256: "A256KW"
                },
                "AES-GCM": {
                    128: "A128GCM",
                    192: "A192GCM",
                    256: "A256GCM"
                },
                "AES-CBC": {
                    128: "A128CBC",
                    192: "A192CBC",
                    256: "A256CBC"
                }
            } [e.name][(e.hash || {}).name || e.length || ""]
        }

        function u(e) {
            (e instanceof ArrayBuffer || e instanceof Uint8Array) && (e = JSON.parse(decodeURIComponent(escape(o(e)))));
            var t = {
                kty: e.kty,
                alg: e.alg,
                ext: e.ext || e.extractable
            };
            switch (t.kty) {
                case "oct":
                    t.k = e.k;
                case "RSA":
                    ["n", "e", "d", "p", "q", "dp", "dq", "qi", "oth"].forEach(function(r) {
                        r in e && (t[r] = e[r])
                    });
                    break;
                default:
                    throw new TypeError("Unsupported key type")
            }
            return t
        }

        function c(e) {
            var t = u(e);
            return S && (t.extractable = t.ext, delete t.ext), n(unescape(encodeURIComponent(JSON.stringify(t)))).buffer
        }

        function s(e) {
            var r = p(e),
                n = !1;
            r.length > 2 && (n = !0, r.shift());
            var i = {
                ext: !0
            };
            switch (r[0][0]) {
                case "1.2.840.113549.1.1.1":
                    var a = ["n", "e", "d", "p", "q", "dp", "dq", "qi"],
                        u = p(r[1]);
                    n && u.shift();
                    for (var c = 0; c < u.length; c++) u[c][0] || (u[c] = u[c].subarray(1)), i[a[c]] = t(o(u[c]));
                    i.kty = "RSA";
                    break;
                default:
                    throw new TypeError("Unsupported key type")
            }
            return i
        }

        function f(e) {
            var t, o = [
                    ["", null]
                ],
                i = !1;
            switch (e.kty) {
                case "RSA":
                    for (var a = ["n", "e", "d", "p", "q", "dp", "dq", "qi"], u = [], c = 0; c < a.length && a[c] in e; c++) {
                        var s = u[c] = n(r(e[a[c]]));
                        128 & s[0] && (u[c] = new Uint8Array(s.length + 1), u[c].set(s, 1))
                    }
                    u.length > 2 && (i = !0, u.unshift(new Uint8Array([0]))), o[0][0] = "1.2.840.113549.1.1.1", t = u;
                    break;
                default:
                    throw new TypeError("Unsupported key type")
            }
            return o.push(new Uint8Array(l(t)).buffer), i ? o.unshift(new Uint8Array([0])) : o[1] = {
                tag: 3,
                value: o[1]
            }, new Uint8Array(l(o)).buffer
        }

        function p(e, t) {
            if (e instanceof ArrayBuffer && (e = new Uint8Array(e)), t || (t = {
                    pos: 0,
                    end: e.length
                }), t.end - t.pos < 2 || t.end > e.length) throw new RangeError("Malformed DER");
            var r = e[t.pos++],
                n = e[t.pos++];
            if (n >= 128) {
                if (n &= 127, t.end - t.pos < n) throw new RangeError("Malformed DER");
                for (var i = 0; n--;) i <<= 8, i |= e[t.pos++];
                n = i
            }
            if (t.end - t.pos < n) throw new RangeError("Malformed DER");
            var a;
            switch (r) {
                case 2:
                    a = e.subarray(t.pos, t.pos += n);
                    break;
                case 3:
                    if (e[t.pos++]) throw new Error("Unsupported bit string");
                    n--;
                case 4:
                    a = new Uint8Array(e.subarray(t.pos, t.pos += n)).buffer;
                    break;
                case 5:
                    a = null;
                    break;
                case 6:
                    var u = btoa(o(e.subarray(t.pos, t.pos += n)));
                    if (!(u in E)) throw new Error("Unsupported OBJECT ID " + u);
                    a = E[u];
                    break;
                case 48:
                    a = [];
                    for (var c = t.pos + n; t.pos < c;) a.push(p(e, t));
                    break;
                default:
                    throw new Error("Unsupported DER tag 0x" + r.toString(16))
            }
            return a
        }

        function l(e, t) {
            t || (t = []);
            var r = 0,
                o = 0,
                i = t.length + 2;
            if (t.push(0, 0), e instanceof Uint8Array) {
                r = 2, o = e.length;
                for (var a = 0; a < o; a++) t.push(e[a])
            } else if (e instanceof ArrayBuffer) {
                r = 4, o = e.byteLength, e = new Uint8Array(e);
                for (var a = 0; a < o; a++) t.push(e[a])
            } else if (null === e) r = 5, o = 0;
            else if ("string" == typeof e && e in K) {
                var u = n(atob(K[e]));
                r = 6, o = u.length;
                for (var a = 0; a < o; a++) t.push(u[a])
            } else if (e instanceof Array) {
                for (var a = 0; a < e.length; a++) l(e[a], t);
                r = 48, o = t.length - i
            } else {
                if (!("object" === (void 0 === e ? "undefined" : v(e)) && 3 === e.tag && e.value instanceof ArrayBuffer)) throw new Error("Unsupported DER value " + e);
                e = new Uint8Array(e.value), r = 3, o = e.byteLength, t.push(0);
                for (var a = 0; a < o; a++) t.push(e[a]);
                o++
            }
            if (o >= 128) {
                var c = o,
                    o = 4;
                for (t.splice(i, 0, c >> 24 & 255, c >> 16 & 255, c >> 8 & 255, 255 & c); o > 1 && !(c >> 24);) c <<= 8, o--;
                o < 4 && t.splice(i, 4 - o), o |= 128
            }
            return t.splice(i - 2, 2, r, o), t
        }

        function y(e, t, r, n) {
            Object.defineProperties(this, {
                _key: {
                    value: e
                },
                type: {
                    value: e.type,
                    enumerable: !0
                },
                extractable: {
                    value: void 0 === r ? e.extractable : r,
                    enumerable: !0
                },
                algorithm: {
                    value: void 0 === t ? e.algorithm : t,
                    enumerable: !0
                },
                usages: {
                    value: void 0 === n ? e.usages : n,
                    enumerable: !0
                }
            })
        }

        function h(e) {
            return "verify" === e || "encrypt" === e || "wrapKey" === e
        }

        function d(e) {
            return "sign" === e || "decrypt" === e || "unwrapKey" === e
        }
        if ("function" != typeof Promise) throw "Promise support required";
        var g = e.crypto || e.msCrypto;
        if (g) {
            var w = g.subtle || g.webkitSubtle;
            if (w) {
                var m = e.Crypto || g.constructor || Object,
                    b = e.SubtleCrypto || w.constructor || Object,
                    A = (e.CryptoKey || e.Key || Object, e.navigator.userAgent.indexOf("Edge/") > -1),
                    S = !!e.msCrypto && !A,
                    k = !g.subtle && !!g.webkitSubtle;
                if (S || k) {
                    var E = {
                            KoZIhvcNAQEB: "1.2.840.113549.1.1.1"
                        },
                        K = {
                            "1.2.840.113549.1.1.1": "KoZIhvcNAQEB"
                        };
                    if (["generateKey", "importKey", "unwrapKey"].forEach(function(e) {
                            var t = w[e];
                            w[e] = function(o, f, p) {
                                var l, m, v, b = [].slice.call(arguments);
                                switch (e) {
                                    case "generateKey":
                                        l = i(o), m = f, v = p;
                                        break;
                                    case "importKey":
                                        l = i(p), m = b[3], v = b[4], "jwk" === o && (f = u(f), f.alg || (f.alg = a(l)), f.key_ops || (f.key_ops = "oct" !== f.kty ? "d" in f ? v.filter(d) : v.filter(h) : v.slice()), b[1] = c(f));
                                        break;
                                    case "unwrapKey":
                                        l = b[4], m = b[5], v = b[6], b[2] = p._key
                                }
                                if ("generateKey" === e && "HMAC" === l.name && l.hash) return l.length = l.length || {
                                    "SHA-1": 512,
                                    "SHA-256": 512,
                                    "SHA-384": 1024,
                                    "SHA-512": 1024
                                } [l.hash.name], w.importKey("raw", g.getRandomValues(new Uint8Array(l.length + 7 >> 3)), l, m, v);
                                if (k && "generateKey" === e && "RSASSA-PKCS1-v1_5" === l.name && (!l.modulusLength || l.modulusLength >= 2048)) return o = i(o), o.name = "RSAES-PKCS1-v1_5", delete o.hash, w.generateKey(o, !0, ["encrypt", "decrypt"]).then(function(e) {
                                    return Promise.all([w.exportKey("jwk", e.publicKey), w.exportKey("jwk", e.privateKey)])
                                }).then(function(e) {
                                    return e[0].alg = e[1].alg = a(l), e[0].key_ops = v.filter(h), e[1].key_ops = v.filter(d), Promise.all([w.importKey("jwk", e[0], l, !0, e[0].key_ops), w.importKey("jwk", e[1], l, m, e[1].key_ops)])
                                }).then(function(e) {
                                    return {
                                        publicKey: e[0],
                                        privateKey: e[1]
                                    }
                                });
                                if ((k || S && "SHA-1" === (l.hash || {}).name) && "importKey" === e && "jwk" === o && "HMAC" === l.name && "oct" === f.kty) return w.importKey("raw", n(r(f.k)), p, b[3], b[4]);
                                if (k && "importKey" === e && ("spki" === o || "pkcs8" === o)) return w.importKey("jwk", s(f), p, b[3], b[4]);
                                if (S && "unwrapKey" === e) return w.decrypt(b[3], p, f).then(function(e) {
                                    return w.importKey(o, e, b[4], b[5], b[6])
                                });
                                var A;
                                try {
                                    A = t.apply(w, b)
                                } catch (e) {
                                    return Promise.reject(e)
                                }
                                return S && (A = new Promise(function(e, t) {
                                    A.onabort = A.onerror = function(e) {
                                        t(e)
                                    }, A.oncomplete = function(t) {
                                        e(t.target.result)
                                    }
                                })), A = A.then(function(e) {
                                    return "HMAC" === l.name && (l.length || (l.length = 8 * e.algorithm.length)), 0 == l.name.search("RSA") && (l.modulusLength || (l.modulusLength = (e.publicKey || e).algorithm.modulusLength), l.publicExponent || (l.publicExponent = (e.publicKey || e).algorithm.publicExponent)), e = e.publicKey && e.privateKey ? {
                                        publicKey: new y(e.publicKey, l, m, v.filter(h)),
                                        privateKey: new y(e.privateKey, l, m, v.filter(d))
                                    } : new y(e, l, m, v)
                                })
                            }
                        }), ["exportKey", "wrapKey"].forEach(function(e) {
                            var r = w[e];
                            w[e] = function(i, c, s) {
                                var p = [].slice.call(arguments);
                                switch (e) {
                                    case "exportKey":
                                        p[1] = c._key;
                                        break;
                                    case "wrapKey":
                                        p[1] = c._key, p[2] = s._key
                                }
                                if ((k || S && "SHA-1" === (c.algorithm.hash || {}).name) && "exportKey" === e && "jwk" === i && "HMAC" === c.algorithm.name && (p[0] = "raw"), !k || "exportKey" !== e || "spki" !== i && "pkcs8" !== i || (p[0] = "jwk"), S && "wrapKey" === e) return w.exportKey(i, c).then(function(e) {
                                    return "jwk" === i && (e = n(unescape(encodeURIComponent(JSON.stringify(u(e)))))), w.encrypt(p[3], s, e)
                                });
                                var l;
                                try {
                                    l = r.apply(w, p)
                                } catch (e) {
                                    return Promise.reject(e)
                                }
                                return S && (l = new Promise(function(e, t) {
                                    l.onabort = l.onerror = function(e) {
                                        t(e)
                                    }, l.oncomplete = function(t) {
                                        e(t.target.result)
                                    }
                                })), "exportKey" === e && "jwk" === i && (l = l.then(function(e) {
                                    return (k || S && "SHA-1" === (c.algorithm.hash || {}).name) && "HMAC" === c.algorithm.name ? {
                                        kty: "oct",
                                        alg: a(c.algorithm),
                                        key_ops: c.usages.slice(),
                                        ext: !0,
                                        k: t(o(e))
                                    } : (e = u(e), e.alg || (e.alg = a(c.algorithm)), e.key_ops || (e.key_ops = "public" === c.type ? c.usages.filter(h) : "private" === c.type ? c.usages.filter(d) : c.usages.slice()), e)
                                })), !k || "exportKey" !== e || "spki" !== i && "pkcs8" !== i || (l = l.then(function(e) {
                                    return e = f(u(e))
                                })), l
                            }
                        }), ["encrypt", "decrypt", "sign", "verify"].forEach(function(e) {
                            var t = w[e];
                            w[e] = function(r, n, o, a) {
                                if (S && (!o.byteLength || a && !a.byteLength)) throw new Error("Empy input is not allowed");
                                var u = [].slice.call(arguments),
                                    c = i(r);
                                if (S && "decrypt" === e && "AES-GCM" === c.name) {
                                    var s = r.tagLength >> 3;
                                    u[2] = (o.buffer || o).slice(0, o.byteLength - s), r.tag = (o.buffer || o).slice(o.byteLength - s)
                                }
                                u[1] = n._key;
                                var f;
                                try {
                                    f = t.apply(w, u)
                                } catch (e) {
                                    return Promise.reject(e)
                                }
                                return S && (f = new Promise(function(t, r) {
                                    f.onabort = f.onerror = function(e) {
                                        r(e)
                                    }, f.oncomplete = function(r) {
                                        var r = r.target.result;
                                        if ("encrypt" === e && r instanceof AesGcmEncryptResult) {
                                            var n = r.ciphertext,
                                                o = r.tag;
                                            r = new Uint8Array(n.byteLength + o.byteLength), r.set(new Uint8Array(n), 0), r.set(new Uint8Array(o), n.byteLength), r = r.buffer
                                        }
                                        t(r)
                                    }
                                })), f
                            }
                        }), S) {
                        var x = w.digest;
                        w.digest = function(e, t) {
                            if (!t.byteLength) throw new Error("Empy input is not allowed");
                            var r;
                            try {
                                r = x.call(w, e, t)
                            } catch (e) {
                                return Promise.reject(e)
                            }
                            return r = new Promise(function(e, t) {
                                r.onabort = r.onerror = function(e) {
                                    t(e)
                                }, r.oncomplete = function(t) {
                                    e(t.target.result)
                                }
                            })
                        }, e.crypto = Object.create(g, {
                            getRandomValues: {
                                value: function(e) {
                                    return g.getRandomValues(e)
                                }
                            },
                            subtle: {
                                value: w
                            }
                        }), e.CryptoKey = y
                    }
                    k && (g.subtle = w, e.Crypto = m, e.SubtleCrypto = b, e.CryptoKey = y)
                }
            }
        }
    }("undefined" == typeof window ? "undefined" == typeof self ? m : self : window);
    var b = function(e) {
            return btoa(String.fromCharCode.apply(null, new Uint8Array(e)))
        },
        A = function() {
            return {
                encrypt: function(e) {
                    return Promise.resolve(e)
                }
            }
        },
        S = function(e) {
            var t = {
                    name: "RSA-OAEP",
                    hash: {
                        name: "SHA-1"
                    }
                },
                r = c(e.keystore, t);
            return {
                encrypt: function(e) {
                    return r.then(function(r) {
                        return s(t, r, e)
                    })
                }
            }
        },
        k = function(e) {
            var t = {
                    name: "RSA-OAEP",
                    hash: {
                        name: "SHA-256"
                    }
                },
                r = c(e.keystore, t);
            return {
                encrypt: function(e) {
                    return r.then(function(r) {
                        return s(t, r, e)
                    })
                }
            }
        },
        E = 3e4;
        window.flex = {
            version: "0.2.1",
            createToken: g,
            encryptCard: jmcrypt,
            utils: {
                isBrowserSupported: r,
                nativeCryptoSupport: u
            }
        }
    return {
        version: "0.2.1",
        createToken: g,
        encryptCard: jmcrypt,
        utils: {
            isBrowserSupported: r,
            nativeCryptoSupport: u
        }
    }
});