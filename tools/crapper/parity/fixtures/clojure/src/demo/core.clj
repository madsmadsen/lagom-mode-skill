(ns demo.core)

(defn grade [score]
  (cond
    (>= score 90) :a
    (>= score 80) :b
    :else :c))

(defn describe-item [item]
  (when (and (map? item) (:name item))
    (str (:name item))))
