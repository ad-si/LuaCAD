-- Basic example of CSG usage

local union_model = 
  (cube { 15, 15, 15, center = true }
      + sphere({ r = 10 })
  )
  :translate(-24, 0, 0)

local intersection_model = 
  cube({ 15, 15, 15, center = true })
  :intersect(sphere({ r = 10 }))

local difference_model = 
  (cube { 15, 15, 15, center = true }
      - sphere({ r = 10 })
  )
  :translate(24, 0, 0)

render(union_model + intersection_model + difference_model)
