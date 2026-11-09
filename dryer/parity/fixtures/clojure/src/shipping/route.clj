(ns shipping.route)

(defn zero [] 0)

(defn route-cost [legs]
  (let [miles (map :miles legs)
        rates (map :rate legs)
        extra (filter pos? miles)]
    (reduce + 0 (map * miles rates))))
