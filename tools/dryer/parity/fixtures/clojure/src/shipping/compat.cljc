(ns shipping.compat)

(defmacro tagged [tag x]
  `(~tag ~x))

(defn now-ms []
  #?(:clj (System/currentTimeMillis)
     :cljs (js/Date.now)))

(defn kind-of [m]
  (:kind m))
