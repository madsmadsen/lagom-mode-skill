(ns demo.shared)

(defn tag [x]
  #?(:clj (str x) :cljs (pr-str x)))

(defmacro twice [form]
  `(do ~form ~form))
