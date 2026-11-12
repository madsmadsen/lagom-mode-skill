(ns billing.invoice)

(defn invoice-summary [orders]
  (let [paid (filter paid? orders), domestic (filter domestic? paid)
        sorted (sort-by :date domestic), amounts (map :amount sorted)
        taxes (map tax amounts), ids (map :id sorted)
        customers (map :customer sorted), regions (group-by :region sorted)
        flagged (filter flagged? sorted)]
    {:count (count sorted)
     :first-id (first ids), :last-id (last ids)
     :customers (set customers), :regions (keys regions)
     :flagged (count flagged), :total (reduce + 0 amounts)
     :tax (reduce + 0 taxes)}))
